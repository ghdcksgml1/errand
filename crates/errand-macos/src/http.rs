//! Bounded HTTPS through NSURLSession, using the system's proxies and trusted certificates.

use errand_core::ports::{HTTP_BODY_LIMIT, HttpClient, HttpFailure, HttpMethod, HttpRequest, HttpResponse};
use objc2::rc::{Retained, autoreleasepool};
use objc2::runtime::{NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{AnyThread, DefinedClass, define_class, msg_send};
use objc2_foundation::{
    NSData, NSError, NSHTTPURLResponse, NSMutableURLRequest, NSString, NSURL, NSURLRequest, NSURLRequestCachePolicy, NSURLResponse, NSURLSession,
    NSURLSessionConfiguration, NSURLSessionDataDelegate, NSURLSessionDataTask, NSURLSessionDelegate, NSURLSessionResponseDisposition,
    NSURLSessionTask, NSURLSessionTaskDelegate,
};
use std::sync::{Mutex, PoisonError, mpsc};
use std::time::Duration;

struct Transfer {
    response: Option<HttpResponse>,
    sender: Option<mpsc::Sender<Result<HttpResponse, HttpFailure>>>,
}
impl Transfer {
    fn finish(&mut self, failed: bool) {
        if let Some(sender) = self.sender.take() {
            let response = self.response.take();
            let _ = sender.send(if failed { Err(HttpFailure) } else { response.ok_or(HttpFailure) });
        }
    }
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "ErrandBoundedResponse"]
    #[ivars = Mutex<Transfer>]
    struct BoundedResponse;

    unsafe impl NSObjectProtocol for BoundedResponse {}
    unsafe impl NSURLSessionDelegate for BoundedResponse {}

    unsafe impl NSURLSessionTaskDelegate for BoundedResponse {
        // The bearer token must never follow a redirect, including to another host.
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

        #[unsafe(method(URLSession:task:didCompleteWithError:))]
        fn finished(&self, _session: &NSURLSession, _task: &NSURLSessionTask, error: Option<&NSError>) {
            self.ivars().lock().unwrap_or_else(PoisonError::into_inner).finish(error.is_some());
        }
    }

    unsafe impl NSURLSessionDataDelegate for BoundedResponse {
        #[unsafe(method(URLSession:dataTask:didReceiveResponse:completionHandler:))]
        fn headers(
            &self,
            _session: &NSURLSession,
            _task: &NSURLSessionDataTask,
            response: &NSURLResponse,
            completion: &block2::DynBlock<dyn Fn(NSURLSessionResponseDisposition)>,
        ) {
            let mut transfer = self.ivars().lock().unwrap_or_else(PoisonError::into_inner);
            let http = response.downcast_ref::<NSHTTPURLResponse>();
            let allow = transfer.sender.is_some() && http.is_some() && response.expectedContentLength() <= HTTP_BODY_LIMIT as i64;
            if allow {
                let http = http.expect("HTTP response checked above");
                transfer.response = Some(HttpResponse {
                    status: u16::try_from(http.statusCode()).unwrap_or(0), body: Vec::new(),
                    retry_after: http.valueForHTTPHeaderField(&NSString::from_str("Retry-After")).map(|value| value.to_string()),
                });
            } else {
                transfer.finish(true);
            }
            drop(transfer);
            completion.call((if allow { NSURLSessionResponseDisposition::Allow } else { NSURLSessionResponseDisposition::Cancel },));
        }

        #[unsafe(method(URLSession:dataTask:didReceiveData:))]
        fn bytes(&self, _session: &NSURLSession, task: &NSURLSessionDataTask, data: &NSData) {
            let mut transfer = self.ivars().lock().unwrap_or_else(PoisonError::into_inner);
            if transfer.sender.is_none() { return; }
            let fits = transfer.response.as_ref().is_some_and(|response| data.len() <= HTTP_BODY_LIMIT.saturating_sub(response.body.len()));
            if fits {
                // NSURLSession gives decoded bytes, so compressed/chunked responses obey the same limit.
                // Check before making any owned copy of the chunk.
                transfer.response.as_mut().expect("response checked above").body.extend_from_slice(&data.to_vec());
            } else {
                transfer.finish(true);
                drop(transfer);
                task.cancel();
            }
        }
    }
);

impl BoundedResponse {
    fn new(sender: mpsc::Sender<Result<HttpResponse, HttpFailure>>) -> Retained<Self> {
        let this = Self::alloc().set_ivars(Mutex::new(Transfer { response: None, sender: Some(sender) }));
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
                &url, NSURLRequestCachePolicy::ReloadIgnoringLocalCacheData, request.timeout.as_secs_f64(),
            );
            outgoing.setHTTPMethod(&NSString::from_str(match request.method {
                HttpMethod::Get => "GET", HttpMethod::Post => "POST", HttpMethod::Put => "PUT", HttpMethod::Delete => "DELETE",
            }));
            for (field, value) in &request.headers {
                outgoing.setValue_forHTTPHeaderField(Some(&NSString::from_str(value)), &NSString::from_str(field));
            }
            if let Some(body) = &request.body { outgoing.setHTTPBody(Some(&NSData::with_bytes(body))); }
            let (sender, receiver) = mpsc::channel();
            let delegate = BoundedResponse::new(sender);
            let configuration = NSURLSessionConfiguration::ephemeralSessionConfiguration();
            configuration.setURLCache(None);
            configuration.setHTTPCookieStorage(None);
            let session = unsafe {
                NSURLSession::sessionWithConfiguration_delegate_delegateQueue(&configuration, Some(ProtocolObject::from_ref(&*delegate)), None)
            };
            let task = session.dataTaskWithRequest(&outgoing);
            task.resume();
            let answer = receiver.recv_timeout(request.timeout + Duration::from_secs(5)).unwrap_or(Err(HttpFailure));
            session.invalidateAndCancel();
            answer
        })
    }
}
