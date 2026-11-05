//! HTTPS through NSURLSession, so macOS's proxies and trusted certificates apply as in every other app.

use block2::RcBlock;
use errand_core::ports::{HttpClient, HttpFailure, HttpMethod, HttpRequest, HttpResponse};
use objc2::rc::{Retained, autoreleasepool};
use objc2::runtime::{NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{AnyThread, define_class, msg_send};
use objc2_foundation::{
    NSData, NSError, NSHTTPURLResponse, NSMutableURLRequest, NSString, NSURL, NSURLRequest, NSURLRequestCachePolicy, NSURLResponse, NSURLSession,
    NSURLSessionConfiguration, NSURLSessionDelegate, NSURLSessionTask, NSURLSessionTaskDelegate,
};
use std::sync::mpsc;
use std::time::Duration;

define_class!(
    // The bearer token must never follow a redirect to another host.
    #[unsafe(super(NSObject))]
    #[name = "ErrandNoRedirects"]
    struct NoRedirects;

    unsafe impl NSObjectProtocol for NoRedirects {}

    unsafe impl NSURLSessionDelegate for NoRedirects {}

    unsafe impl NSURLSessionTaskDelegate for NoRedirects {
        #[unsafe(method(URLSession:task:willPerformHTTPRedirection:newRequest:completionHandler:))]
        fn refuse_redirect(
            &self,
            _session: &NSURLSession,
            _task: &NSURLSessionTask,
            _response: &NSHTTPURLResponse,
            _request: &NSURLRequest,
            completion: &block2::DynBlock<dyn Fn(*mut NSURLRequest)>,
        ) {
            completion.call((std::ptr::null_mut(),));
        }
    }
);

impl NoRedirects {
    fn new() -> Retained<NoRedirects> {
        let this = NoRedirects::alloc().set_ivars(());
        unsafe { msg_send![super(this), init] }
    }
}

/// One ephemeral session per request: no cookies, no cache, nothing kept.
pub struct UrlSession;

impl HttpClient for UrlSession {
    fn send(&self, request: &HttpRequest) -> Result<HttpResponse, HttpFailure> {
        autoreleasepool(|_| {
            let url = NSURL::URLWithString(&NSString::from_str(&request.url)).ok_or(HttpFailure)?;
            let outgoing = NSMutableURLRequest::requestWithURL_cachePolicy_timeoutInterval(
                &url,
                NSURLRequestCachePolicy::ReloadIgnoringLocalCacheData,
                request.timeout.as_secs_f64(),
            );
            outgoing.setHTTPMethod(&NSString::from_str(match request.method {
                HttpMethod::Get => "GET",
                HttpMethod::Post => "POST",
                HttpMethod::Put => "PUT",
                HttpMethod::Delete => "DELETE",
            }));
            for (field, value) in &request.headers {
                outgoing.setValue_forHTTPHeaderField(Some(&NSString::from_str(value)), &NSString::from_str(field));
            }
            if let Some(body) = &request.body {
                outgoing.setHTTPBody(Some(&NSData::with_bytes(body)));
            }
            let delegate = NoRedirects::new();
            let session = unsafe {
                NSURLSession::sessionWithConfiguration_delegate_delegateQueue(
                    &NSURLSessionConfiguration::ephemeralSessionConfiguration(),
                    Some(ProtocolObject::from_ref(&*delegate)),
                    None,
                )
            };
            let (sender, receiver) = mpsc::channel();
            let finished = RcBlock::new(move |data: *mut NSData, response: *mut NSURLResponse, error: *mut NSError| {
                // SAFETY: NSURLSession passes valid objects or null, for the duration of this call.
                let response = unsafe { response.as_ref() };
                let answer = match (error.is_null(), response.and_then(|response| response.downcast_ref::<NSHTTPURLResponse>())) {
                    (true, Some(http)) => Ok(HttpResponse {
                        status: u16::try_from(http.statusCode()).unwrap_or(0),
                        body: unsafe { data.as_ref() }.map(NSData::to_vec).unwrap_or_default(),
                        retry_after: http.valueForHTTPHeaderField(&NSString::from_str("Retry-After")).map(|value| value.to_string()),
                    }),
                    _ => Err(HttpFailure),
                };
                let _ = sender.send(answer);
            });
            // SAFETY: the block only sends on a channel, which is safe from NSURLSession's queue.
            let task = unsafe { session.dataTaskWithRequest_completionHandler(&outgoing, &finished) };
            task.resume();
            let answer = receiver.recv_timeout(request.timeout + Duration::from_secs(5)).unwrap_or(Err(HttpFailure));
            session.invalidateAndCancel();
            answer
        })
    }
}
