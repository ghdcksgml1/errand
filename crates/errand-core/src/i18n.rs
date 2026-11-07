//! Every word Errand shows, in Korean and English. The app speaks the first of the person's preferred
//! languages it knows, and English when it knows none of them. What the services and apps report, such as
//! session titles or a model's name, is shown as it came.

use crate::avatar::{Avatar, Eyes, Mouth, Outfit, Part, Shape, Top};
use crate::model::{Alert, Notice, OpenError};
use crate::provider::Provider;
use crate::rooms::{MAX_ROOMS, Presence, RoomError, Visibility};
use crate::sessions::SessionState;
use crate::sessions::text::PICTURE;
use crate::usage::{Mood, Span, UsageError, WindowName};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Language {
    Korean,
    English,
}

impl Language {
    pub const ALL: [Language; 2] = [Language::Korean, Language::English];

    /// The first of these language tags, most preferred first, that Errand speaks; English otherwise.
    pub fn preferred<T: AsRef<str>>(tags: impl IntoIterator<Item = T>) -> Language {
        tags.into_iter().find_map(|tag| Language::from_tag(tag.as_ref())).unwrap_or(Language::English)
    }

    /// `ko`, `ko-KR`, `en_US` and the like; None for a language Errand does not speak.
    pub fn from_tag(tag: &str) -> Option<Language> {
        match tag.trim().split(['-', '_']).next()?.to_ascii_lowercase().as_str() {
            "ko" => Some(Language::Korean),
            "en" => Some(Language::English),
            _ => None,
        }
    }

    /// The tag `--language` takes for it.
    pub fn tag(self) -> &'static str {
        self.pick("ko", "en")
    }

    fn pick(self, korean: &'static str, english: &'static str) -> &'static str {
        match self {
            Language::Korean => korean,
            Language::English => english,
        }
    }

    // Times.

    pub fn not_recorded(self) -> &'static str {
        self.pick("기록 대기", "Not recorded yet")
    }

    pub fn just_now(self) -> &'static str {
        self.pick("방금", "just now")
    }

    pub fn minutes_ago(self, minutes: i64) -> String {
        match self {
            Language::Korean => format!("{minutes}분 전"),
            Language::English => format!("{minutes} min ago"),
        }
    }

    pub fn hours_ago(self, hours: i64) -> String {
        match self {
            Language::Korean => format!("{hours}시간 전"),
            Language::English => format!("{hours} hr ago"),
        }
    }

    /// How long running work has been going.
    pub fn elapsed(self, minutes: i64) -> String {
        match (self, minutes) {
            (_, ..1) => self.pick("방금 시작", "just started").to_owned(),
            (Language::Korean, 1..60) => format!("{minutes}분째"),
            (Language::English, 1..60) => format!("{minutes} min"),
            (Language::Korean, _) => format!("{}시간 {}분째", minutes / 60, minutes % 60),
            (Language::English, _) => format!("{} hr {} min", minutes / 60, minutes % 60),
        }
    }

    // Limits.

    /// "주간", "Codex 5시간", "주간 Fable"; "Weekly", "Codex 5-hour", "Weekly Fable".
    pub fn window_title(self, name: &WindowName) -> String {
        let span = match (self, name.span) {
            (_, Span::Minutes(10080)) => self.pick("주간", "Weekly").to_owned(),
            (Language::Korean, Span::Minutes(minutes)) if minutes % 1440 == 0 => format!("{}일", minutes / 1440),
            (Language::English, Span::Minutes(minutes)) if minutes % 1440 == 0 => format!("{}-day", minutes / 1440),
            (Language::Korean, Span::Minutes(minutes)) if minutes % 60 == 0 => format!("{}시간", minutes / 60),
            (Language::English, Span::Minutes(minutes)) if minutes % 60 == 0 => format!("{}-hour", minutes / 60),
            (Language::Korean, Span::Minutes(minutes)) => format!("{minutes}분"),
            (Language::English, Span::Minutes(minutes)) => format!("{minutes}-minute"),
            (Language::Korean, Span::Numbered(place)) => format!("한도 {place}"),
            (Language::English, Span::Numbered(place)) => format!("Limit {place}"),
        };
        [name.bucket.as_deref(), Some(span.as_str()), name.model.as_deref()].into_iter().flatten().collect::<Vec<_>>().join(" ")
    }

    /// "주간 72% 남음", or that the share left is unknown.
    pub fn remaining(self, title: &str, left: Option<i64>) -> String {
        match (self, left) {
            (Language::Korean, Some(left)) => format!("{title} {left}% 남음"),
            (Language::English, Some(left)) => format!("{title} {left}% left"),
            (Language::Korean, None) => format!("{title} 남은 비율 확인 불가"),
            (Language::English, None) => format!("{title} remaining unknown"),
        }
    }

    /// "72% 남음" on its own.
    pub fn percent_left(self, left: i64) -> String {
        match self {
            Language::Korean => format!("{left}% 남음"),
            Language::English => format!("{left}% left"),
        }
    }

    /// "10/19 09:00 초기화" beside a battery.
    pub fn resets(self, date: Option<&str>) -> String {
        match (self, date) {
            (Language::Korean, Some(date)) => format!("{date} 초기화"),
            (Language::English, Some(date)) => format!("resets {date}"),
            (_, None) => self.pick("초기화 확인 불가", "reset unknown").to_owned(),
        }
    }

    /// "주간 초기화 10/19 09:00" in the menu's tooltip.
    pub fn window_reset(self, title: &str, date: Option<&str>) -> String {
        match (self, date) {
            (Language::Korean, Some(date)) => format!("{title} 초기화 {date}"),
            (Language::English, Some(date)) => format!("{title} resets {date}"),
            (Language::Korean, None) => format!("{title} 초기화 시간 확인 불가"),
            (Language::English, None) => format!("{title} reset time unknown"),
        }
    }

    pub fn no_usage_data(self) -> &'static str {
        self.pick("사용량 정보 없음", "No usage data")
    }

    pub fn unknown(self) -> &'static str {
        self.pick("확인 불가", "Unknown")
    }

    pub fn checking(self) -> &'static str {
        self.pick("확인 중", "Checking")
    }

    pub fn checking_reset(self) -> &'static str {
        self.pick("초기화 확인 중", "Checking reset")
    }

    /// Beside values kept from the last read after a later one failed.
    pub fn last_known(self) -> &'static str {
        self.pick("이전 값", "Last known")
    }

    pub fn check_failed(self) -> &'static str {
        self.pick("조회 실패", "Check failed")
    }

    /// Stands in for a limit's name when there is none to give.
    pub fn usage(self) -> &'static str {
        self.pick("사용량", "Usage")
    }

    pub fn usage_error(self, error: &UsageError) -> String {
        match (self, error) {
            (_, UsageError::ClaudeNotHeard) => self
                .pick(
                    "새로고침을 누르면 Claude Code에서 받아와요.\n터미널에서 쓴다면 python3 scripts/connect_claude.py로\n자동으로 받을 수 있어요.",
                    "Press refresh to get it from Claude Code.\nIf you use it in Terminal, python3 scripts/connect_claude.py\nkeeps it up to date.",
                )
                .to_owned(),
            (_, UsageError::ClaudeAskFailed) => self
                .pick(
                    "Claude Code에서 사용량을 받지 못했습니다.\nClaude 데스크톱 앱에 로그인돼 있는지 확인하세요.",
                    "Couldn't get usage from Claude Code.\nCheck that the Claude desktop app is signed in.",
                )
                .to_owned(),
            (_, UsageError::CodexMissing) => self.pick("공식 Codex 앱을 찾지 못했습니다.", "Couldn't find the Codex app.").to_owned(),
            (Language::Korean, UsageError::CodexLaunch(reason)) => format!("Codex를 실행하지 못했습니다: {reason}"),
            (Language::English, UsageError::CodexLaunch(reason)) => format!("Couldn't start Codex: {reason}"),
            (_, UsageError::CodexClosed) => self.pick("Codex API 연결이 종료됐습니다.", "The connection to Codex closed.").to_owned(),
            (Language::Korean, UsageError::CodexFailed(method)) => format!("Codex API 요청에 실패했습니다 ({method})."),
            (Language::English, UsageError::CodexFailed(method)) => format!("Codex refused the request ({method})."),
            (Language::Korean, UsageError::CodexNoAnswer(method)) => format!("Codex API 응답을 받지 못했습니다 ({method})."),
            (Language::English, UsageError::CodexNoAnswer(method)) => format!("Codex didn't answer ({method})."),
            (_, UsageError::CodexSignIn) => self.pick("해당 계정으로 로그인한 뒤 사용량을 조회하세요.", "Sign in to Codex, then check usage again.").to_owned(),
        }
    }

    // Batteries and the dock.

    /// "Codex 남은 사용량", what VoiceOver calls a battery.
    pub fn usage_left(self, provider: Provider) -> String {
        match self {
            Language::Korean => format!("{} 남은 사용량", provider.title()),
            Language::English => format!("{} usage left", provider.title()),
        }
    }

    /// A battery's tooltip: the service, what is left, the next reset, the clock it is read in, and any failure.
    pub fn battery_tooltip(self, provider: Provider, summary: &str, reset: &str, computer: &str, failure: Option<&str>) -> String {
        let failure = failure.map(|failure| format!("\n{failure}")).unwrap_or_default();
        match self {
            Language::Korean => format!("{} · {summary} · 다음 {reset} · {computer} 시간대 기준{failure}", provider.title()),
            Language::English => format!("{} · {summary} · {reset} · {computer} time zone{failure}", provider.title()),
        }
    }

    pub fn profile_label(self, name: &str) -> String {
        match self {
            Language::Korean => format!("프로필 {name}"),
            Language::English => format!("Profile {name}"),
        }
    }

    pub fn dock_tooltip(self) -> &'static str {
        self.pick("클릭해서 사용량 자세히 보기 · 드래그해서 이동", "Click for usage details · Drag to move")
    }

    pub fn dock_label(self) -> &'static str {
        self.pick("남은 사용량", "Usage left")
    }

    /// The person's character on the dock, as VoiceOver names it.
    pub fn edit_avatar(self) -> &'static str {
        self.pick("캐릭터 꾸미기", "Edit character")
    }

    pub fn avatar_tooltip(self) -> &'static str {
        self.pick("클릭해서 캐릭터 꾸미기", "Click to edit your character")
    }

    pub fn avatar_editor_title(self) -> &'static str {
        self.pick("내 캐릭터", "My character")
    }

    /// The editor's button that draws a whole new character.
    pub fn shuffle_avatar(self) -> &'static str {
        self.pick("무작위", "Shuffle")
    }

    pub fn avatar_part(self, part: Part) -> &'static str {
        match part {
            Part::Shape => self.pick("몸", "Body"),
            Part::Top => self.pick("머리 위", "Top"),
            Part::Eyes => self.pick("눈", "Eyes"),
            Part::Mouth => self.pick("입", "Mouth"),
            Part::Outfit => self.pick("옷", "Clothes"),
            Part::Cheeks => self.pick("볼", "Cheeks"),
        }
    }

    /// What one part of a character is now.
    pub fn avatar_choice(self, avatar: &Avatar, part: Part) -> &'static str {
        match part {
            Part::Shape => match avatar.shape {
                Shape::Dome => self.pick("동글", "Round"),
                Shape::Square => self.pick("네모", "Square"),
                Shape::Triangle => self.pick("세모", "Triangle"),
                Shape::Cloud => self.pick("몽글", "Cloud"),
                Shape::Hexagon => self.pick("육각", "Hexagon"),
            },
            Part::Top => match avatar.top {
                Top::Bare => self.pick("없음", "None"),
                Top::BearEars => self.pick("곰 귀", "Bear ears"),
                Top::CatEars => self.pick("고양이 귀", "Cat ears"),
                Top::Sprout => self.pick("새싹", "Sprout"),
                Top::Beanie => self.pick("털모자", "Beanie"),
            },
            Part::Eyes => match avatar.eyes {
                Eyes::Dots => self.pick("점", "Dots"),
                Eyes::Sparkly => self.pick("반짝", "Sparkly"),
                Eyes::Happy => self.pick("웃는 눈", "Smiling"),
                Eyes::Sleepy => self.pick("졸린 눈", "Sleepy"),
                Eyes::Squint => self.pick("질끈", "Squinting"),
            },
            Part::Mouth => match avatar.mouth {
                Mouth::Smile => self.pick("미소", "Smile"),
                Mouth::Cat => self.pick("고양이", "Cat"),
                Mouth::Round => self.pick("동그란 입", "Round"),
                Mouth::Grin => self.pick("활짝", "Grin"),
                Mouth::Flat => self.pick("일자", "Flat"),
            },
            Part::Outfit => match avatar.outfit {
                Outfit::Tee => self.pick("티셔츠", "T-shirt"),
                Outfit::Stripes => self.pick("줄무늬", "Stripes"),
                Outfit::Collar => self.pick("카라티", "Collar"),
                Outfit::Hoodie => self.pick("후드티", "Hoodie"),
                Outfit::Overalls => self.pick("멜빵", "Overalls"),
            },
            Part::Cheeks => {
                if avatar.blush {
                    self.pick("볼터치", "Blush")
                } else {
                    self.pick("없음", "None")
                }
            }
        }
    }

    /// The arrows either side of a part, for VoiceOver: "몸 이전", "Next body".
    pub fn previous_choice(self, part: Part) -> String {
        match self {
            Language::Korean => format!("{} 이전", self.avatar_part(part)),
            Language::English => format!("Previous {}", self.avatar_part(part).to_lowercase()),
        }
    }

    pub fn next_choice(self, part: Part) -> String {
        match self {
            Language::Korean => format!("{} 다음", self.avatar_part(part)),
            Language::English => format!("Next {}", self.avatar_part(part).to_lowercase()),
        }
    }

    /// The clothes' colours, in `avatar::CLOTHES` order.
    pub fn clothes_colour(self, index: usize) -> &'static str {
        const NAMES: [(&str, &str); 12] = [
            ("파랑", "Blue"),
            ("하늘", "Sky"),
            ("청록", "Teal"),
            ("초록", "Green"),
            ("연두", "Lime"),
            ("노랑", "Yellow"),
            ("주황", "Orange"),
            ("빨강", "Red"),
            ("분홍", "Pink"),
            ("보라", "Purple"),
            ("남색", "Indigo"),
            ("회색", "Slate"),
        ];
        NAMES.get(index).map_or("", |&(korean, english)| self.pick(korean, english))
    }

    /// "Codex와 Claude", "Codex and Claude", or the one service shown.
    fn services(self, shown: &[Provider]) -> String {
        let names: Vec<&str> = shown.iter().map(|provider| provider.title()).collect();
        names.join(self.pick("와 ", " and "))
    }

    pub fn refresh_usage(self) -> &'static str {
        self.pick("사용량 새로고침", "Refresh usage")
    }

    pub fn refresh_all(self) -> &'static str {
        self.pick("모든 계정 사용량 새로고침", "Refresh usage for all accounts")
    }

    pub fn refresh(self) -> &'static str {
        self.pick("새로고침", "Refresh")
    }

    // The task header and cards.

    pub fn tasks_title(self, count: usize) -> String {
        match self {
            Language::Korean => format!("내가 시킨 작업 {count}"),
            Language::English => format!("My tasks {count}"),
        }
    }

    /// One count beside the task title: new or stalled, running, or seen.
    pub fn pill(self, state: SessionState, count: usize) -> String {
        match (self, state) {
            (Language::Korean, SessionState::Running) => format!("진행 중 {count}개"),
            (Language::Korean, SessionState::Seen) => format!("확인함 {count}개"),
            (Language::Korean, _) => format!("확인할 것 {count}개"),
            (Language::English, SessionState::Running) => format!("{count} running"),
            (Language::English, SessionState::Seen) => format!("{count} seen"),
            (Language::English, _) => format!("{count} to check"),
        }
    }

    pub fn tasks_label(self, count: usize, pills: &[&str], collapsed: bool) -> String {
        let pills = if pills.is_empty() { String::new() } else { format!(", {}", pills.join(", ")) };
        match self {
            Language::Korean => format!("내가 시킨 작업 {count}개{pills}. {}", self.fold_word(collapsed)),
            Language::English => format!("My tasks, {count}{pills}. {}", self.fold_word(collapsed)),
        }
    }

    /// The word beside the header's chevron: what a click does.
    pub fn fold_word(self, collapsed: bool) -> &'static str {
        if collapsed { self.pick("펼치기", "Show") } else { self.pick("접기", "Hide") }
    }

    pub fn fold_tooltip(self, collapsed: bool) -> &'static str {
        if collapsed {
            self.pick("클릭해서 작업 카드 펼치기", "Click to show the task cards")
        } else {
            self.pick("클릭해서 작업 카드 접기 · 카드는 체크해야 사라져요", "Click to hide the task cards · A card leaves only once checked off")
        }
    }

    pub fn session_state(self, state: SessionState) -> &'static str {
        match state {
            SessionState::New => self.pick("새 결과", "New"),
            SessionState::Seen => self.pick("확인함", "Seen"),
            SessionState::Running => self.pick("진행 중", "Running"),
            SessionState::Stalled => self.pick("응답 없음", "Stalled"),
            SessionState::Aborted => self.pick("중단됨", "Stopped"),
            SessionState::Unknown => self.pick("상태 확인 불가", "Unknown"),
            SessionState::Done => self.pick("완료", "Done"),
        }
    }

    /// A session that has no title of its own.
    pub fn untitled(self, provider: Provider) -> &'static str {
        match provider {
            Provider::Codex => self.pick("제목 없는 세션", "Untitled session"),
            Provider::Claude => self.pick("Claude 세션", "Claude session"),
        }
    }

    /// Recorded text as the person reads it: a picture in a message is named, not shown.
    pub fn recorded(self, text: &str) -> String {
        text.replace(PICTURE, self.pick("[이미지]", "[Image]"))
    }

    pub fn prompt_unreadable(self) -> &'static str {
        self.pick("요청 내용을 읽지 못했어요", "Couldn't read the request")
    }

    /// "7분 전 요청" on running work, "7분 전 응답" once answered.
    pub fn card_time(self, when: &str, answered: bool) -> String {
        match (self, answered) {
            (Language::Korean, false) => format!("{when} 요청"),
            (Language::Korean, true) => format!("{when} 응답"),
            (Language::English, false) => format!("asked {when}"),
            (Language::English, true) => format!("answered {when}"),
        }
    }

    pub fn complete_label(self, title: &str) -> String {
        match self {
            Language::Korean => format!("{title} 완료"),
            Language::English => format!("Complete {title}"),
        }
    }

    pub fn complete_tooltip(self, checked: bool) -> &'static str {
        if checked {
            self.pick("완료를 취소합니다", "Undo completing")
        } else {
            self.pick("완료로 정리합니다. 같은 세션에 새로 요청하면 다시 나타나요.", "Mark done and clear it. A new request in the session brings it back.")
        }
    }

    pub fn undo_label(self, title: &str) -> String {
        match self {
            Language::Korean => format!("{title} 완료 되돌리기"),
            Language::English => format!("Undo completing {title}"),
        }
    }

    pub fn undo(self) -> &'static str {
        self.pick("되돌리기", "Undo")
    }

    pub fn result_line(self, result: &str) -> String {
        match self {
            Language::Korean => format!("결과: {result}"),
            Language::English => format!("Result: {result}"),
        }
    }

    /// What the app says to do next, in a tooltip.
    pub fn to_do_line(self, action: &str) -> String {
        match self {
            Language::Korean => format!("할 일: {action}"),
            Language::English => format!("To do: {action}"),
        }
    }

    /// What the app says to do next, on the card itself.
    pub fn to_do(self, action: &str) -> String {
        match self {
            Language::Korean => format!("할 일 · {action}"),
            Language::English => format!("To do · {action}"),
        }
    }

    pub fn click_to_open(self) -> &'static str {
        self.pick("클릭해서 열기", "Click to open")
    }

    pub fn open(self) -> &'static str {
        self.pick("열기", "Open")
    }

    pub fn opens_session(self) -> &'static str {
        self.pick("누르면 세션을 엽니다", "Opens the session")
    }

    /// What VoiceOver reads for a card: the app, the title, its state, the request, and on a full card what happened.
    pub fn card_label(self, app: &str, title: &str, chip: &str, prompt: &str, detail: Option<&str>) -> String {
        let detail = detail.map(|detail| format!(", {detail}")).unwrap_or_default();
        match self {
            Language::Korean => format!("{app}, {title}, {chip}, 요청: {prompt}{detail}"),
            Language::English => format!("{app}, {title}, {chip}, request: {prompt}{detail}"),
        }
    }

    /// What VoiceOver reads for a sub-task's line, after `card_label`.
    pub fn sub_task_label(self, card: &str) -> String {
        match self {
            Language::Korean => format!("하위 작업, {card}"),
            Language::English => format!("Sub-task, {card}"),
        }
    }

    pub fn stalled_line(self) -> &'static str {
        self.pick("20분 넘게 기록이 없어요 · 열어서 확인하세요", "Nothing new for over 20 minutes · Open it to check")
    }

    pub fn working(self, elapsed: &str) -> String {
        match self {
            Language::Korean => format!("작업 중 · {elapsed}"),
            Language::English => format!("Working · {elapsed}"),
        }
    }

    pub fn stopped_line(self) -> &'static str {
        self.pick("작업을 중단했어요", "Stopped")
    }

    pub fn answered_line(self) -> &'static str {
        self.pick("응답을 마쳤어요", "Answered")
    }

    pub fn clears_soon(self) -> &'static str {
        self.pick("잠시 뒤 목록에서 정리돼요", "Leaves the list in a moment")
    }

    /// The bar under cards that do not fit.
    pub fn more_below(self, count: usize) -> String {
        match self {
            Language::Korean => format!("{count}개 더 있어요"),
            Language::English => format!("{count} more"),
        }
    }

    pub fn more_below_label(self, count: usize) -> String {
        match self {
            Language::Korean => format!("아래로 작업 {count}개 더 보기"),
            Language::English => format!("Show {count} more tasks below"),
        }
    }

    pub fn back_to_top(self) -> &'static str {
        self.pick("맨 위로", "Back to top")
    }

    pub fn back_to_top_label(self) -> &'static str {
        self.pick("맨 위 작업으로", "Back to the top task")
    }

    // Account cards, the details and the account window.

    pub fn sign_in_needed(self, provider: Provider) -> &'static str {
        match provider {
            Provider::Codex => self.pick("Codex 로그인 필요", "Sign in to Codex"),
            Provider::Claude => self.pick("Claude Code 로그인 필요", "Sign in to Claude Code"),
        }
    }

    pub fn running_status(self, label: &str) -> String {
        match self {
            Language::Korean => format!("{label} · 실행 중"),
            Language::English => format!("{label} · open"),
        }
    }

    /// The card's button: bring the open app forward, or open it.
    pub fn open_button(self, running: bool) -> &'static str {
        if running { self.pick("창 보기", "Show") } else { self.pick("계정 열기", "Open") }
    }

    pub fn open_label(self, provider: Provider) -> String {
        match self {
            Language::Korean => format!("{} 열기", provider.title()),
            Language::English => format!("Open {}", provider.title()),
        }
    }

    pub fn next_reset(self, date: Option<&str>) -> String {
        let date = date.unwrap_or(self.unknown());
        match self {
            Language::Korean => format!("다음 초기화   {date}"),
            Language::English => format!("Next reset   {date}"),
        }
    }

    pub fn reset_seen(self, date: &str) -> String {
        match self {
            Language::Korean => format!("초기화 감지   {date}"),
            Language::English => format!("Reset seen   {date}"),
        }
    }

    pub fn reset_seen_tooltip(self) -> &'static str {
        self.pick(
            "앱이 초기화를 감지한 시각입니다. 기록 전의 초기화 시각은 추측하지 않습니다.",
            "When Errand saw this limit refill. Refills before it started recording are not guessed.",
        )
    }

    /// A card without limits: a headline and what to do, for a failure, a read in progress or no data.
    pub fn no_limits(self, provider: Provider, failure: Option<String>, loading: bool) -> (String, String) {
        match failure {
            Some(failure) => (self.pick("사용량을 가져오지 못했어요", "Couldn't get usage").to_owned(), failure),
            None if loading => (self.pick("사용량을 확인하고 있어요", "Checking usage").to_owned(), self.pick("잠시만 기다려주세요.", "Just a moment.").to_owned()),
            None if provider == Provider::Claude => (
                self.pick("사용량 정보가 없어요", "No usage yet").to_owned(),
                self.pick("새로고침을 누르거나 터미널에서\nClaude Code를 쓰면 표시돼요.", "Press refresh, or use Claude Code\nin Terminal, to see it here.").to_owned(),
            ),
            None => (
                self.pick("사용량 정보가 없어요", "No usage yet").to_owned(),
                self.pick("해당 계정으로 로그인한 뒤\n다시 확인해주세요.", "Sign in to this account,\nthen check again.").to_owned(),
            ),
        }
    }

    pub fn update_failed(self) -> &'static str {
        self.pick("갱신 실패 · 이전 값", "Update failed · last known")
    }

    pub fn updating(self) -> &'static str {
        self.pick("갱신 중…", "Updating…")
    }

    pub fn no_data(self) -> &'static str {
        self.pick("정보 없음", "No data")
    }

    pub fn up_to_date(self) -> &'static str {
        self.pick("최신 사용량", "Up to date")
    }

    pub fn checked_at(self, status: &str, time: &str) -> String {
        match self {
            Language::Korean => format!("{status} · 조회 {time}"),
            Language::English => format!("{status} · checked {time}"),
        }
    }

    /// The time Claude Code last spoke, which is as fresh as its limits are.
    pub fn reported_at(self, time: &str) -> String {
        match self {
            Language::Korean => format!("Claude Code 기준 {time}"),
            Language::English => format!("From Claude Code, {time}"),
        }
    }

    pub fn limit_count(self, status: &str, count: usize) -> String {
        match self {
            Language::Korean => format!("{status} · {count}개 한도"),
            Language::English => format!("{status} · {count} limits"),
        }
    }

    pub fn checking_usage(self) -> &'static str {
        self.pick("사용량 조회 중…", "Checking usage…")
    }

    pub fn details_title(self) -> &'static str {
        self.pick("우리 계정 사용량", "Account usage")
    }

    pub fn show_all(self) -> &'static str {
        self.pick("전체 보기", "Show all")
    }

    pub fn close(self) -> &'static str {
        self.pick("닫기", "Close")
    }

    pub fn window_title_usage(self) -> &'static str {
        self.pick("Errand · 사용량", "Errand · Usage")
    }

    pub fn tray_window_title(self) -> &'static str {
        self.pick("Errand · 내가 시킨 작업", "Errand · My tasks")
    }

    pub fn dashboard_title(self, shown: &[Provider]) -> String {
        match self {
            Language::Korean => format!("{} 사용량", self.services(shown)),
            Language::English => format!("{} usage", self.services(shown)),
        }
    }

    pub fn dashboard_subtitle(self) -> &'static str {
        self.pick("남은 사용량과 초기화 시간을 한눈에 확인하세요.", "What's left and when it refills, at a glance.")
    }

    pub fn mood(self, mood: Mood) -> &'static str {
        match mood {
            Mood::Healthy => self.pick("아직 여유가 있어요", "Plenty left"),
            Mood::Low => self.pick("잠깐 쉬어가도 좋아요", "Maybe take a short break"),
            Mood::Reset => self.pick("새 한도가 열렸어요", "A fresh limit just opened"),
            Mood::Unknown => self.pick("사용량을 확인해주세요", "Check your usage"),
        }
    }

    pub fn waiting_to_connect(self) -> &'static str {
        self.pick("연결되면 다시 알려드릴게요", "Will update once it connects")
    }

    pub fn waiting_for_usage(self) -> &'static str {
        self.pick("사용량을 기다리고 있어요", "Waiting for usage")
    }

    pub fn checking_all(self) -> &'static str {
        self.pick("모든 계정 사용량을 조회하는 중…", "Checking usage for all accounts…")
    }

    pub fn update_rhythm(self, computer: &str) -> String {
        match self {
            Language::Korean => format!("2분마다 갱신 · 배터리는 가장 적게 남은 한도 기준 · {computer} 시간대"),
            Language::English => format!("Updates every 2 minutes · Batteries show the lowest limit · {computer} time zone"),
        }
    }

    pub fn notice(self, notice: &Notice, shown: &[Provider]) -> String {
        match (self, notice) {
            (Language::Korean, Notice::Checking) => format!("{}의 사용량을 확인합니다.", self.services(shown)),
            (Language::English, Notice::Checking) => format!("Checking {} usage.", self.services(shown)),
            (Language::Korean, Notice::Opening(provider)) => format!("{} 창을 여는 중…", provider.title()),
            (Language::English, Notice::Opening(provider)) => format!("Opening {}…", provider.title()),
            (Language::Korean, Notice::Opened(provider)) => format!("{} 창을 열었습니다.", provider.title()),
            (Language::English, Notice::Opened(provider)) => format!("Opened {}.", provider.title()),
            (_, Notice::OpenFailed(error)) => self.open_error(error),
        }
    }

    pub fn open_error(self, error: &OpenError) -> String {
        match error {
            OpenError::NotInstalled(Provider::Codex) => {
                self.pick("Applications 폴더에 Codex 또는 ChatGPT 앱을 설치하세요.", "Install the Codex or ChatGPT app in the Applications folder.").to_owned()
            }
            OpenError::NotInstalled(Provider::Claude) => self.pick("Applications 폴더에 Claude 앱을 설치하세요.", "Install the Claude app in the Applications folder.").to_owned(),
            OpenError::System(reason) => reason.clone(),
        }
    }

    /// An alert's title and text.
    pub fn alert(self, alert: &Alert) -> (String, String) {
        match alert {
            Alert::OpenFailed(error) => (self.pick("계정 창을 열지 못했습니다", "Couldn't open the app").to_owned(), self.open_error(error)),
            Alert::MigrationFailed(reason) => (self.pick("Dual Codex 기록을 옮기지 못했습니다", "Couldn't move the Dual Codex records").to_owned(), reason.clone()),
        }
    }

    pub fn ok(self) -> &'static str {
        self.pick("확인", "OK")
    }

    // Work rooms.

    pub fn rooms_title(self) -> &'static str {
        self.pick("작업방", "Work rooms")
    }

    pub fn rooms_subtitle(self) -> &'static str {
        self.pick(
            "공개 방에서는 익명으로, 비공개 방에서는 내 프로필로 함께해요. 무슨 작업인지는 절대 공유하지 않아요.",
            "Public rooms are anonymous; private rooms show your profile. What anyone works on is never shared.",
        )
    }

    /// Why no more rooms can be joined, at the list's top and on its buttons.
    pub fn rooms_full(self) -> String {
        match self {
            Language::Korean => format!("방은 {MAX_ROOMS}개까지 들어갈 수 있어요. 다른 방을 나가면 들어가요."),
            Language::English => format!("You can be in {MAX_ROOMS} rooms at once. Leave one to join another."),
        }
    }

    pub fn visibility(self, visibility: Visibility) -> &'static str {
        match visibility {
            Visibility::Public => self.pick("익명 참여", "Anonymous"),
            Visibility::Private => self.pick("프로필 공개", "Profile shown"),
        }
    }

    pub fn room_people(self, members: usize) -> String {
        match self {
            Language::Korean => format!("{members}명"),
            Language::English => format!("{members} people"),
        }
    }

    pub fn working_words(self, working: usize) -> String {
        match self {
            Language::Korean => format!("지금 {working}명 작업 중"),
            Language::English => format!("{working} working now"),
        }
    }

    /// Over the rooms the person is in, with how many of the most they can be in.
    pub fn joined_rooms(self, joined: usize) -> String {
        match self {
            Language::Korean => format!("참여 중  {joined}/{MAX_ROOMS}"),
            Language::English => format!("Joined  {joined}/{MAX_ROOMS}"),
        }
    }

    /// Over the rooms the person is not in.
    pub fn other_rooms(self) -> &'static str {
        self.pick("다른 방", "Other rooms")
    }

    /// Beside a joined room's name.
    pub fn joined_badge(self) -> &'static str {
        self.pick("참여 중", "Joined")
    }

    pub fn join_room(self) -> &'static str {
        self.pick("들어가기", "Join")
    }

    pub fn leave_room(self) -> &'static str {
        self.pick("나가기", "Leave")
    }

    pub fn presence(self, presence: Presence) -> &'static str {
        match presence {
            Presence::Working => self.pick("작업 중", "Working"),
            Presence::Resting => self.pick("쉬는 중", "On a break"),
            Presence::Away => self.pick("자리 비움", "Away"),
        }
    }

    /// A room's tab, for VoiceOver: the room, who may see it, and how many are in it.
    pub fn room_tab_label(self, name: &str, visibility: Visibility, people: Option<usize>) -> String {
        let people = people.map(|people| format!(", {}", self.room_people(people))).unwrap_or_default();
        format!("{name}, {}{people}", self.visibility(visibility))
    }

    /// Past the rows a room shows on the dock.
    pub fn more_members(self, more: usize) -> String {
        match self {
            Language::Korean => format!("외 {more}명"),
            Language::English => format!("and {more} more"),
        }
    }

    /// How long someone has been in a room, for VoiceOver beside the stopwatch on their row.
    pub fn time_in_room(self, span: jiff::SignedDuration) -> String {
        let (hours, minutes, seconds) = crate::format::clock_parts(span);
        let (hour, minute, second) = match self {
            Language::Korean => ("시간", "분", "초"),
            Language::English => (" h", " min", " s"),
        };
        let mut parts = Vec::new();
        if hours > 0 {
            parts.push(format!("{hours}{hour}"));
        }
        if hours > 0 || minutes > 0 {
            parts.push(format!("{minutes}{minute}"));
        }
        parts.push(format!("{seconds}{second}"));
        match self {
            Language::Korean => format!("들어온 지 {}", parts.join(" ")),
            Language::English => format!("Here for {}", parts.join(" ")),
        }
    }

    /// The person themselves in a room.
    pub fn me(self) -> &'static str {
        self.pick("나", "You")
    }

    /// Under the list of rooms: what they hear, or that they are sample data.
    pub fn rooms_footer(self, sample: bool) -> &'static str {
        match sample {
            true => self.pick("예시 데이터로 보고 있어요. 무슨 작업인지는 어떤 방에도 공유되지 않아요.", "These are sample rooms. No room ever sees what you work on."),
            false => self.pick(
                "방에는 작업 개수 세 가지와 캐릭터, 비공개 방에는 닉네임까지만 보내요. 무슨 작업인지는 공유되지 않아요.",
                "Rooms hear your three task counts and character, private ones your nickname too, never what you work on.",
            ),
        }
    }

    pub fn rooms_loading(self) -> &'static str {
        self.pick("방 목록을 불러오는 중…", "Loading rooms…")
    }

    pub fn room_joining(self) -> &'static str {
        self.pick("들어가는 중…", "Joining…")
    }

    pub fn room_error(self, error: RoomError) -> &'static str {
        match error {
            RoomError::Unreachable => self.pick("작업방 서버에 연결하지 못했어요.", "Couldn't reach the rooms server."),
            RoomError::Unauthorized => self.pick("작업방 서버가 이 Mac을 알아보지 못했어요.", "The rooms server didn't recognize this Mac."),
            RoomError::NotFound => self.pick("이 방은 이제 없어요.", "This room is gone."),
            RoomError::Full => self.pick("방이 꽉 찼어요. 자리가 나면 들어가요.", "The room is full. You'll get in when someone leaves."),
            RoomError::TooManyRooms => self.pick("방에 너무 많이 들어가 있어요. 다른 방을 나가면 들어가요.", "You're in too many rooms. Leave one to get in."),
            RoomError::RateLimited => self.pick("작업방 서버가 잠시 쉬어 달래요. 곧 다시 이어져요.", "The rooms server asked for a break. Back soon."),
            RoomError::ClientTooOld => self.pick("작업방을 쓰려면 Errand를 업데이트하세요.", "Update Errand to use work rooms."),
            RoomError::Rejected => self.pick("작업방 서버가 요청을 받지 않았어요.", "The rooms server turned the request down."),
        }
    }

    /// A room's tab: the room, and what a click on it does.
    pub fn room_tab_tooltip(self, name: &str, tagline: &str, open: bool) -> String {
        let about = if tagline.is_empty() { name.to_owned() } else { format!("{name} · {tagline}") };
        let click = match open {
            false => self.pick("눌러서 함께 있는 사람 보기", "Click to see who's here"),
            true => self.pick("눌러서 접기", "Click to fold"),
        };
        format!("{about}\n{click}")
    }

    pub fn rooms_item(self) -> &'static str {
        self.pick("작업방", "Work Rooms")
    }

    /// The way to the rooms on the dock: a capsule beside the nickname outside them, a plus after the tabs in them.
    pub fn rooms_button(self) -> &'static str {
        self.pick("작업방", "Rooms")
    }

    pub fn rooms_button_tooltip(self) -> &'static str {
        self.pick("작업방 둘러보기 · 들어가기", "Browse and join work rooms")
    }

    pub fn rooms_plus_tooltip(self) -> &'static str {
        self.pick("작업방 둘러보기 · 들어가기 · 나가기", "Browse, join and leave work rooms")
    }

    // The menu bar.

    pub fn open_app_item(self, provider: Provider, email: Option<&str>) -> String {
        let email = email.map(|email| format!(" · {email}")).unwrap_or_default();
        match self {
            Language::Korean => format!("{} 열기{email}", provider.title()),
            Language::English => format!("Open {}{email}", provider.title()),
        }
    }

    pub fn usage_check_failed(self) -> &'static str {
        self.pick("사용량 조회 실패", "Couldn't check usage")
    }

    pub fn refresh_item(self) -> &'static str {
        self.pick("사용량 새로고침", "Refresh Usage")
    }

    pub fn dashboard_item(self) -> &'static str {
        self.pick("계정 창 열기", "Open Usage Window")
    }

    pub fn widget_item(self, visible: bool) -> &'static str {
        if visible { self.pick("플로팅 위젯 숨기기", "Hide Floating Widget") } else { self.pick("플로팅 위젯 보이기", "Show Floating Widget") }
    }

    pub fn reset_widget_item(self) -> &'static str {
        self.pick("플로팅 위젯 위치 초기화", "Reset Widget Position")
    }

    /// Checked while the service shows.
    pub fn show_service_item(self, provider: Provider) -> String {
        match self {
            Language::Korean => format!("{} 표시", provider.title()),
            Language::English => format!("Show {}", provider.title()),
        }
    }

    pub fn service_toggle_tooltip(self, provider: Provider, shown: bool) -> String {
        match (self, shown) {
            (Language::Korean, true) => format!("쓰지 않으면 끄세요. {}의 배터리·카드·작업을 숨기고 사용량을 조회하지 않습니다.", provider.title()),
            (Language::Korean, false) => format!("{}의 배터리·카드·작업을 다시 보여줍니다.", provider.title()),
            (Language::English, true) => format!("Turn off if you don't use it: hides {}'s battery, card and tasks and stops reading its usage.", provider.title()),
            (Language::English, false) => format!("Shows {}'s battery, card and tasks again.", provider.title()),
        }
    }

    /// The update button, short enough to sit beside the nickname.
    pub fn update_offer(self, version: &str) -> String {
        match self {
            Language::Korean => format!("새 버전 {version}"),
            Language::English => format!("Version {version}"),
        }
    }

    pub fn update_item(self, version: &str) -> String {
        match self {
            Language::Korean => format!("Errand {version}로 업데이트"),
            Language::English => format!("Update to Errand {version}"),
        }
    }

    pub fn update_tooltip(self, version: &str, installs: bool) -> String {
        match (self, installs) {
            (Language::Korean, true) => format!("눌러서 Errand {version}로 업데이트해요. 앱이 다시 시작돼요."),
            (Language::English, true) => format!("Click to update to Errand {version}. The app restarts."),
            (Language::Korean, false) => format!("눌러서 Errand {version} 릴리스 페이지를 열어요."),
            (Language::English, false) => format!("Click to open the Errand {version} release page."),
        }
    }

    pub fn updating_app(self) -> &'static str {
        self.pick("업데이트하는 중…", "Updating…")
    }

    pub fn update_again(self) -> &'static str {
        self.pick("다시 시도", "Try again")
    }

    pub fn update_again_item(self) -> &'static str {
        self.pick("업데이트 다시 시도", "Try the update again")
    }

    pub fn install_failed(self, version: &str) -> String {
        match self {
            Language::Korean => format!("Errand {version}로 업데이트하지 못했어요. 지금 앱은 그대로예요."),
            Language::English => format!("Couldn't update to Errand {version}. This copy is unchanged."),
        }
    }

    pub fn quit_item(self) -> &'static str {
        self.pick("Errand 종료", "Quit Errand")
    }

    /// The menu bar battery's tooltip, per service.
    pub fn status_level(self, provider: Provider, left: Option<i64>) -> String {
        match (self, left) {
            (_, Some(left)) => self.remaining(provider.title(), Some(left)),
            (Language::Korean, None) => format!("{} 확인 중", provider.title()),
            (Language::English, None) => format!("{} checking", provider.title()),
        }
    }

    /// Said on systems the app has no screens for yet.
    pub fn no_screens(self) -> &'static str {
        self.pick("이 OS의 화면은 아직 없습니다.", "has no screens for this OS yet.")
    }
}
