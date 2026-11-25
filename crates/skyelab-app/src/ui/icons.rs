use gpui::SharedString;
use gpui_component::IconNamed;

// Keep the type, lookup paths and embedded artwork in one registry.
macro_rules! icons {
    ($($name:ident => $file:literal,)*) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub(crate) enum SkyeIcon {
            $($name,)*
        }

        impl IconNamed for SkyeIcon {
            fn path(self) -> SharedString {
                match self {
                    $(Self::$name => concat!("icons/skyelab/", $file, ".svg"),)*
                }.into()
            }
        }

        pub(super) const PATHS: &[&str] = &[
            $(concat!("icons/skyelab/", $file, ".svg"),)*
        ];

        pub(super) fn bytes(path: &str) -> Option<&'static [u8]> {
            match path {
                $(concat!("icons/skyelab/", $file, ".svg") =>
                    Some(include_bytes!(concat!("../../icons/ui/", $file, ".svg"))),)*
                _ => None,
            }
        }
    };
}

icons! {
    AlarmClock => "alarm-clock",
    AlarmClockOff => "alarm-clock-off",
    Archive => "archive",
    ArrowDown => "arrow-down",
    ArrowRight => "arrow-right",
    ArrowUp => "arrow-up",
    Ask => "ask",
    Asterisk => "asterisk",
    Bookmark => "bookmark",
    Bot => "bot",
    Browser => "browser",
    ChartNoAxesColumn => "chart-no-axes-column",
    ChartPie => "chart-pie",
    Check => "check",
    ChevronDown => "chevron-down",
    ChevronLeft => "chevron-left",
    ChevronRight => "chevron-right",
    ChevronUp => "chevron-up",
    ChevronsUpDown => "chevrons-up-down",
    CircleAlert => "circle-alert",
    CircleCheck => "circle-check",
    CircleDot => "circle-dot",
    CircleUser => "circle-user",
    CircleX => "circle-x",
    ClipboardList => "clipboard-list",
    Clock => "clock",
    Close => "close",
    CloudUpload => "cloud-upload",
    Code => "code",
    Collapse => "collapse",
    Columns2 => "columns2",
    Copy => "copy",
    Delete => "delete",
    Expand => "expand",
    ExternalLink => "external-link",
    File => "file",
    FileDiff => "file-diff",
    FileText => "file-text",
    Files => "files",
    Filter => "filter",
    Folder => "folder",
    FolderGit2 => "folder-git2",
    FolderOpen => "folder-open",
    FolderPlus => "folder-plus",
    GitBranch => "git-branch",
    GitCommitVertical => "git-commit-vertical",
    GitMerge => "git-merge",
    GitPullRequest => "git-pull-request",
    GitPullRequestClosed => "git-pull-request-closed",
    GitPullRequestDraft => "git-pull-request-draft",
    Github => "github",
    Globe => "globe",
    Image => "image",
    Info => "info",
    Keyboard => "keyboard",
    Link => "link",
    LoaderCircle => "loader-circle",
    Lock => "lock",
    Map => "map",
    MessageCircle => "message-circle",
    MessagesSquare => "messages-square",
    Mic => "mic",
    Minus => "minus",
    Model => "model",
    Monitor => "monitor",
    Moon => "moon",
    NewThread => "new-thread",
    Palette => "palette",
    PanelBottom => "panel-bottom",
    PanelLeft => "panel-left",
    PanelRight => "panel-right",
    Paperclip => "paperclip",
    PenLine => "pen-line",
    Pilcrow => "pilcrow",
    Pin => "pin",
    Play => "play",
    Plug => "plug",
    Plus => "plus",
    Quote => "quote",
    RefreshCw => "refresh-cw",
    Remote => "remote",
    Rows3 => "rows3",
    Search => "search",
    Settings => "settings",
    Settings2 => "settings2",
    ShieldOff => "shield-off",
    Smartphone => "smartphone",
    Sparkles => "sparkles",
    Square => "square",
    SquareCode => "square-code",
    SquareMenu => "square-menu",
    SquareTerminal => "square-terminal",
    Sun => "sun",
    Terminal => "terminal",
    TextWrap => "text-wrap",
    Trash => "trash",
    TriangleAlert => "triangle-alert",
    Undo2 => "undo2",
    Unlink => "unlink",
    User => "user",
    UserCheck => "user-check",
    UserRoundX => "user-round-x",
    Workflow => "workflow",
    X => "x",
}
