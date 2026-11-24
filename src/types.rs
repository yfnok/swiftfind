//! Shared result types.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Command,
    Setting,
    App,
    File,
    Folder,
    Calc,
    Web,
    Clip,
    Snippet,
    Window,
    Process,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Command => "命令",
            Kind::Setting => "设置",
            Kind::App => "应用",
            Kind::File => "文件",
            Kind::Folder => "文件夹",
            Kind::Calc => "计算",
            Kind::Web => "网页",
            Kind::Clip => "剪贴板",
            Kind::Snippet => "片段",
            Kind::Window => "窗口",
            Kind::Process => "进程",
        }
    }
}

#[derive(Clone, Debug)]
pub enum Action {
    /// ShellExecute a file, folder, URL or URI with optional arguments.
    Open {
        target: String,
        args: String,
        admin: bool,
        cwd: Option<String>,
    },
    /// Copy text to the clipboard (calculator result).
    Copy(String),
    /// Paste a clipboard-history item (text or image) into the previously active window.
    PasteClip { id: i64, image: bool },
    /// Paste a text snippet (variables like `{date}` are expanded at paste time).
    PasteText(String),
    /// Bring an existing top-level window to the foreground.
    ActivateWindow(isize),
    /// Terminate a process selected through the `kill` query.
    KillProcess { pid: u32, name: String },
    /// Quit the app.
    Quit,
    /// Open this app's settings panel.
    OpenPanel,
}

#[derive(Clone, Debug)]
pub struct Item {
    /// Stable key used for learning (path, URI, command name …).
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub kind: Kind,
    pub action: Action,
    /// Filesystem path if the item is a real file/folder/shortcut.
    pub path: Option<String>,
    pub score: f32,
}

impl Item {
    pub fn can_run_admin(&self) -> bool {
        matches!(
            self.kind,
            Kind::App | Kind::File | Kind::Command | Kind::Setting
        ) && matches!(self.action, Action::Open { .. })
    }
}
