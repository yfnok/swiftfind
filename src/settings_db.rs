//! Windows 10/11 settings and system tools index.
//!
//! Each entry: (Chinese title, English title, launch target, keywords).
//! Keywords are space separated; `|` separates multi-word phrases. Include pinyin initials
//! and common synonyms so both `hjbl` and `env` find "环境变量".
//!
//! Launch target kinds:
//!   * `ms-settings:...`       → opened through ShellExecute (Settings app pages)
//!   * `cmd:program|args`      → program with args (ShellExecute)
//!   * anything else           → program / .msc / .cpl / shell: path, no args

pub struct SettingDef {
    pub zh: &'static str,
    pub en: &'static str,
    pub target: &'static str,
    pub kw: &'static str,
    /// Needs elevation to be useful (e.g. editing system variables).
    pub admin: bool,
}

const fn s(
    zh: &'static str,
    en: &'static str,
    target: &'static str,
    kw: &'static str,
) -> SettingDef {
    SettingDef {
        zh,
        en,
        target,
        kw,
        admin: false,
    }
}
const fn a(
    zh: &'static str,
    en: &'static str,
    target: &'static str,
    kw: &'static str,
) -> SettingDef {
    SettingDef {
        zh,
        en,
        target,
        kw,
        admin: true,
    }
}

pub static SETTINGS: &[SettingDef] = &[
    // ───────── 系统 System ─────────
    s("设置", "Settings", "ms-settings:", "shezhi sz settings"),
    s("系统", "System", "ms-settings:system", "xitong xt"),
    s(
        "显示",
        "Display",
        "ms-settings:display",
        "xianshi xs 屏幕 分辨率 缩放 resolution scale monitor 显示器",
    ),
    s(
        "夜间模式",
        "Night light",
        "ms-settings:nightlight",
        "yejian yjms 护眼 蓝光 night light",
    ),
    s(
        "HDR",
        "HDR",
        "ms-settings:display-advancedgraphics",
        "hdr 高动态范围",
    ),
    s(
        "图形设置",
        "Graphics settings",
        "ms-settings:display-advancedgraphics",
        "tuxing txsz graphics gpu 显卡 独显",
    ),
    s(
        "高级显示 / 刷新率",
        "Advanced display",
        "ms-settings:display-advanced",
        "shuaxinlv 刷新率 refresh rate hz",
    ),
    s(
        "声音",
        "Sound",
        "ms-settings:sound",
        "shengyin sy 音量 扬声器 麦克风 audio volume speaker microphone",
    ),
    s(
        "声音控制面板",
        "Sound control panel",
        "mmsys.cpl",
        "playback recording 录音 播放 设备 mmsys",
    ),
    s(
        "音量混合器",
        "Volume mixer",
        "ms-settings:apps-volume",
        "yinliang hunheqi volume mixer 应用音量",
    ),
    s(
        "通知",
        "Notifications",
        "ms-settings:notifications",
        "tongzhi tz notification 通知",
    ),
    s(
        "专注助手 / 勿扰",
        "Focus assist",
        "ms-settings:quiethours",
        "zhuanzhu 勿扰 do not disturb focus",
    ),
    s(
        "电源和电池",
        "Power & battery",
        "ms-settings:powersleep",
        "dianyuan dy 睡眠 休眠 电池 power sleep battery 节电",
    ),
    s(
        "电源选项 (控制面板)",
        "Power options",
        "powercfg.cpl",
        "dianyuan xuanxiang 电源计划 power plan 高性能",
    ),
    s(
        "存储",
        "Storage",
        "ms-settings:storagesense",
        "cunchu cc 磁盘清理 storage sense 空间",
    ),
    s(
        "附近共享",
        "Nearby sharing",
        "ms-settings:crossdevice",
        "fujin gongxiang nearby share",
    ),
    s(
        "多任务处理",
        "Multitasking",
        "ms-settings:multitasking",
        "duorenwu 贴靠 snap 虚拟桌面 alt tab",
    ),
    s(
        "激活",
        "Activation",
        "ms-settings:activation",
        "jihuo jh activation 许可证 license 密钥",
    ),
    s(
        "疑难解答",
        "Troubleshoot",
        "ms-settings:troubleshoot",
        "yinan jieda 故障排除 troubleshoot",
    ),
    s(
        "恢复",
        "Recovery",
        "ms-settings:recovery",
        "huifu hf 重置此电脑 reset pc recovery 还原",
    ),
    s(
        "投影到此电脑",
        "Projecting to this PC",
        "ms-settings:project",
        "touying 投屏 project miracast",
    ),
    s(
        "远程桌面",
        "Remote Desktop",
        "ms-settings:remotedesktop",
        "yuancheng zhuomian 远程 rdp remote desktop",
    ),
    s(
        "剪贴板",
        "Clipboard",
        "ms-settings:clipboard",
        "jiantieban jtb clipboard 剪切板 历史",
    ),
    s(
        "关于 / 系统信息",
        "About",
        "ms-settings:about",
        "guanyu gy 设备名称 重命名 about 配置 规格 device name",
    ),
    s(
        "系统信息 (msinfo32)",
        "System Information",
        "msinfo32.exe",
        "msinfo32 xitong xinxi 硬件信息",
    ),
    s(
        "DirectX 诊断",
        "DirectX Diagnostic",
        "dxdiag.exe",
        "dxdiag 显卡 directx",
    ),
    s(
        "系统属性",
        "System Properties",
        "SystemPropertiesAdvanced.exe",
        "xitong shuxing 高级系统设置 advanced system sysdm",
    ),
    s(
        "性能选项",
        "Performance Options",
        "SystemPropertiesPerformance.exe",
        "xingneng 视觉效果 虚拟内存 页面文件 performance visual effects",
    ),
    s(
        "系统保护 / 还原点",
        "System Protection",
        "SystemPropertiesProtection.exe",
        "huanyuan 还原点 restore point 系统保护",
    ),
    s(
        "远程设置 (系统属性)",
        "Remote settings",
        "SystemPropertiesRemote.exe",
        "yuancheng 远程协助",
    ),
    a(
        "环境变量",
        "Environment Variables",
        "cmd:rundll32.exe|sysdm.cpl,EditEnvironmentVariables",
        "huanjing bianliang hjbl env path PATH 系统变量 用户变量 environment variables",
    ),
    s(
        "用户环境变量",
        "User environment variables",
        "cmd:rundll32.exe|sysdm.cpl,EditEnvironmentVariables",
        "yonghu huanjing bianliang user env",
    ),
    // ───────── 蓝牙和设备 Devices ─────────
    s(
        "蓝牙和设备",
        "Bluetooth & devices",
        "ms-settings:bluetooth",
        "lanya ly bluetooth 设备 配对",
    ),
    s(
        "打印机和扫描仪",
        "Printers & scanners",
        "ms-settings:printers",
        "dayinji dyj printer scanner 打印",
    ),
    s(
        "设备和打印机 (控制面板)",
        "Devices and Printers",
        "control.exe printers",
        "devices printers 打印机",
    ),
    s(
        "鼠标",
        "Mouse",
        "ms-settings:mousetouchpad",
        "shubiao sb mouse 指针速度 滚轮",
    ),
    s(
        "鼠标属性 (控制面板)",
        "Mouse properties",
        "main.cpl",
        "shubiao shuxing mouse 双击速度 指针",
    ),
    s(
        "触摸板",
        "Touchpad",
        "ms-settings:devices-touchpad",
        "chumoban touchpad 手势",
    ),
    s(
        "键盘",
        "Keyboard",
        "ms-settings:keyboard",
        "jianpan jp keyboard",
    ),
    s(
        "输入 / 打字",
        "Typing",
        "ms-settings:typing",
        "shuru dazi typing 自动更正 拼写 输入",
    ),
    s("USB", "USB", "ms-settings:usb", "usb 接口"),
    s(
        "自动播放",
        "AutoPlay",
        "ms-settings:autoplay",
        "zidong bofang autoplay",
    ),
    s(
        "相机",
        "Cameras",
        "ms-settings:camera",
        "xiangji camera 摄像头 webcam",
    ),
    s(
        "手写笔和 Windows Ink",
        "Pen & Windows Ink",
        "ms-settings:pen",
        "shouxiebi pen ink",
    ),
    s(
        "设备管理器",
        "Device Manager",
        "devmgmt.msc",
        "shebei guanliqi sbgl 驱动 driver device manager",
    ),
    s(
        "添加硬件向导",
        "Add hardware wizard",
        "hdwwiz.cpl",
        "tianjia yingjian",
    ),
    s(
        "游戏控制器",
        "Game controllers",
        "joy.cpl",
        "youxi shoubing 手柄 joystick gamepad",
    ),
    // ───────── 网络和 Internet ─────────
    s(
        "网络和 Internet",
        "Network & internet",
        "ms-settings:network",
        "wangluo wl network internet 上网",
    ),
    s(
        "Wi-Fi",
        "Wi-Fi",
        "ms-settings:network-wifi",
        "wifi wlan 无线 wifi 无线网络",
    ),
    s(
        "管理已知网络",
        "Manage known networks",
        "ms-settings:network-wifisettings",
        "wifi 已知网络 忘记网络",
    ),
    s(
        "以太网",
        "Ethernet",
        "ms-settings:network-ethernet",
        "yitaiwang ethernet 有线网络",
    ),
    s("VPN", "VPN", "ms-settings:network-vpn", "vpn 虚拟专用网络"),
    s(
        "飞行模式",
        "Airplane mode",
        "ms-settings:network-airplanemode",
        "feixing moshi airplane flight",
    ),
    s(
        "移动热点",
        "Mobile hotspot",
        "ms-settings:network-mobilehotspot",
        "redian hotspot 热点 共享网络",
    ),
    s(
        "代理",
        "Proxy",
        "ms-settings:network-proxy",
        "daili dl proxy 代理服务器",
    ),
    s(
        "拨号",
        "Dial-up",
        "ms-settings:network-dialup",
        "bohao dial",
    ),
    s(
        "数据使用量",
        "Data usage",
        "ms-settings:datausage",
        "shuju shiyongliang 流量 data usage",
    ),
    s(
        "高级网络设置",
        "Advanced network settings",
        "ms-settings:network-advancedsettings",
        "gaoji wangluo 网络重置 network reset",
    ),
    s(
        "网络连接 (适配器)",
        "Network connections",
        "ncpa.cpl",
        "wangluo shipeiqi ncpa 网卡 适配器 adapter ip dns 网络连接",
    ),
    s(
        "网络和共享中心",
        "Network and Sharing Center",
        "control.exe /name Microsoft.NetworkAndSharingCenter",
        "wangluo gongxiang zhongxin sharing center",
    ),
    s(
        "Internet 选项",
        "Internet Options",
        "inetcpl.cpl",
        "internet xuanxiang ie 代理 证书 inetcpl",
    ),
    s(
        "Windows 防火墙",
        "Windows Defender Firewall",
        "firewall.cpl",
        "fanghuoqiang fhq firewall 防火墙 防火墙设置",
    ),
    s(
        "高级安全 Windows 防火墙",
        "Windows Firewall with Advanced Security",
        "wf.msc",
        "fanghuoqiang gaoji 入站规则 出站规则 inbound outbound rule firewall 端口 port",
    ),
    s(
        "允许应用通过防火墙",
        "Allow an app through firewall",
        "control.exe /name Microsoft.WindowsFirewall /page pageConfigureApps",
        "fanghuoqiang yunxu yingyong allow app firewall",
    ),
    s(
        "Windows 安全中心 - 防火墙",
        "Windows Security - Firewall",
        "windowsdefender://network",
        "anquan zhongxin 防火墙 network protection",
    ),
    // ───────── 个性化 Personalization ─────────
    s(
        "个性化",
        "Personalization",
        "ms-settings:personalization",
        "gexinghua gxh personalization 主题",
    ),
    s(
        "背景 / 壁纸",
        "Background",
        "ms-settings:personalization-background",
        "beijing bj 壁纸 桌面背景 wallpaper background",
    ),
    s(
        "颜色 / 深色模式",
        "Colors",
        "ms-settings:colors",
        "yanse ys 深色 浅色 dark mode light 主题色 accent color",
    ),
    s(
        "主题",
        "Themes",
        "ms-settings:themes",
        "zhuti zt theme 桌面图标 desktop icons",
    ),
    s(
        "锁屏界面",
        "Lock screen",
        "ms-settings:lockscreen",
        "suoping sp lock screen 锁屏",
    ),
    s(
        "开始菜单",
        "Start",
        "ms-settings:personalization-start",
        "kaishi caidan ksc start menu 开始",
    ),
    s(
        "任务栏",
        "Taskbar",
        "ms-settings:taskbar",
        "renwulan rwl taskbar 任务栏设置 居中 左对齐",
    ),
    s(
        "字体",
        "Fonts",
        "ms-settings:fonts",
        "ziti zt fonts 安装字体",
    ),
    s(
        "文本光标",
        "Text cursor",
        "ms-settings:easeofaccess-cursor",
        "guangbiao 文本光标 text cursor",
    ),
    s(
        "设备使用情况",
        "Device usage",
        "ms-settings:personalization-device-usage",
        "shebei shiyong",
    ),
    s(
        "桌面图标设置",
        "Desktop icon settings",
        "cmd:rundll32.exe|shell32.dll,Control_RunDLL desk.cpl,,0",
        "zhuomian tubiao 此电脑图标 回收站图标 desktop icons",
    ),
    s(
        "屏幕保护程序",
        "Screen saver",
        "cmd:rundll32.exe|shell32.dll,Control_RunDLL desk.cpl,,1",
        "pingmu baohu screensaver 屏保",
    ),
    // ───────── 应用 Apps ─────────
    s(
        "应用和功能 / 已安装应用",
        "Installed apps",
        "ms-settings:appsfeatures",
        "yingyong gongneng yygn 卸载 uninstall apps features 已安装应用",
    ),
    s(
        "程序和功能 (卸载程序)",
        "Programs and Features",
        "appwiz.cpl",
        "chengxu gongneng 卸载程序 uninstall appwiz programs",
    ),
    s(
        "Windows 功能 (启用或关闭)",
        "Turn Windows features on or off",
        "optionalfeatures.exe",
        "windows gongneng 启用或关闭 hyper-v wsl 虚拟机平台 .net telnet optional features",
    ),
    s(
        "可选功能",
        "Optional features",
        "ms-settings:optionalfeatures",
        "kexuan gongneng optional",
    ),
    s(
        "默认应用",
        "Default apps",
        "ms-settings:defaultapps",
        "moren yingyong 默认浏览器 default apps 文件关联 file association",
    ),
    s(
        "离线地图",
        "Offline maps",
        "ms-settings:maps",
        "lixian ditu maps",
    ),
    s(
        "网站应用",
        "Apps for websites",
        "ms-settings:appsforwebsites",
        "wangzhan yingyong",
    ),
    s(
        "视频播放",
        "Video playback",
        "ms-settings:videoplayback",
        "shipin bofang video hdr",
    ),
    s(
        "启动应用 / 开机自启",
        "Startup apps",
        "ms-settings:startupapps",
        "qidong yingyong kaiji ziqi startup 开机启动 自启动 启动项",
    ),
    s(
        "任务管理器",
        "Task Manager",
        "taskmgr.exe",
        "renwu guanliqi rwgl taskmgr 进程 process 性能 启动项",
    ),
    // ───────── 账户 Accounts ─────────
    s(
        "账户",
        "Accounts",
        "ms-settings:yourinfo",
        "zhanghu zh accounts 你的信息 头像",
    ),
    s(
        "登录选项 / 密码 / PIN",
        "Sign-in options",
        "ms-settings:signinoptions",
        "denglu xuanxiang 密码 pin hello 指纹 face password",
    ),
    s(
        "电子邮件和账户",
        "Email & accounts",
        "ms-settings:emailandaccounts",
        "youjian zhanghu email accounts",
    ),
    s(
        "其他用户 / 家庭",
        "Other users",
        "ms-settings:otherusers",
        "qita yonghu 添加用户 family users",
    ),
    s(
        "Windows 备份",
        "Windows backup",
        "ms-settings:backup",
        "beifen bf backup 备份",
    ),
    s(
        "访问工作或学校账户",
        "Access work or school",
        "ms-settings:workplace",
        "gongzuo xuexiao work school 域",
    ),
    s(
        "用户账户 (netplwiz)",
        "User Accounts",
        "netplwiz.exe",
        "netplwiz 自动登录 auto login 免密码登录 用户账户",
    ),
    s(
        "用户账户 (控制面板)",
        "User Accounts (Control Panel)",
        "control.exe userpasswords2",
        "yonghu zhanghu userpasswords",
    ),
    s(
        "本地用户和组",
        "Local Users and Groups",
        "lusrmgr.msc",
        "bendi yonghu he zu lusrmgr 管理员 administrator 本地账户",
    ),
    s(
        "凭据管理器",
        "Credential Manager",
        "control.exe /name Microsoft.CredentialManager",
        "pingju guanliqi credential 保存的密码 windows 凭据",
    ),
    // ───────── 时间和语言 Time & language ─────────
    s(
        "时间和语言",
        "Time & language",
        "ms-settings:dateandtime",
        "shijian yuyan sjyy time language",
    ),
    s(
        "日期和时间",
        "Date & time",
        "ms-settings:dateandtime",
        "riqi shijian rqsj 时区 timezone 同步时间 sync clock",
    ),
    s(
        "日期和时间 (控制面板)",
        "Date and Time",
        "timedate.cpl",
        "riqi shijian timedate 时钟 附加时钟",
    ),
    s(
        "语言和区域",
        "Language & region",
        "ms-settings:regionlanguage",
        "yuyan quyu yyqy language region 输入法 添加语言 ime",
    ),
    s(
        "区域 (控制面板)",
        "Region",
        "intl.cpl",
        "quyu intl 日期格式 数字格式 管理 非unicode 系统区域",
    ),
    s(
        "语音",
        "Speech",
        "ms-settings:speech",
        "yuyin speech 语音识别",
    ),
    s(
        "输入法设置",
        "IME settings",
        "ms-settings:regionlanguage",
        "shurufa srf ime 微软拼音 pinyin 输入法",
    ),
    // ───────── 游戏 Gaming ─────────
    s(
        "游戏",
        "Gaming",
        "ms-settings:gaming-gamebar",
        "youxi yx gaming",
    ),
    s(
        "Xbox Game Bar",
        "Xbox Game Bar",
        "ms-settings:gaming-gamebar",
        "game bar youxi lan 录屏 xbox",
    ),
    s(
        "游戏模式",
        "Game Mode",
        "ms-settings:gaming-gamemode",
        "youxi moshi game mode",
    ),
    s(
        "屏幕截图",
        "Captures",
        "ms-settings:gaming-gamedvr",
        "jietu 录制 captures dvr",
    ),
    // ───────── 辅助功能 Accessibility ─────────
    s(
        "辅助功能",
        "Accessibility",
        "ms-settings:easeofaccess",
        "fuzhu gongneng fzgn accessibility 轻松使用",
    ),
    s(
        "文本大小",
        "Text size",
        "ms-settings:easeofaccess-display",
        "wenben daxiao text size 字体大小 放大字体",
    ),
    s(
        "放大镜",
        "Magnifier",
        "ms-settings:easeofaccess-magnifier",
        "fangdajing magnifier",
    ),
    s(
        "颜色滤镜",
        "Color filters",
        "ms-settings:easeofaccess-colorfilter",
        "yanse lujing color filter 色盲",
    ),
    s(
        "对比度主题",
        "Contrast themes",
        "ms-settings:easeofaccess-highcontrast",
        "duibidu contrast",
    ),
    s(
        "讲述人",
        "Narrator",
        "ms-settings:easeofaccess-narrator",
        "jiangshuren narrator 屏幕阅读",
    ),
    s(
        "屏幕键盘",
        "On-screen keyboard",
        "osk.exe",
        "pingmu jianpan osk 虚拟键盘 on-screen keyboard",
    ),
    s(
        "字幕",
        "Captions",
        "ms-settings:easeofaccess-closedcaptioning",
        "zimu captions",
    ),
    s(
        "语音访问",
        "Voice access",
        "ms-settings:easeofaccess-voiceaccess",
        "yuyin fangwen voice access",
    ),
    s(
        "粘滞键 / 筛选键",
        "Sticky keys",
        "ms-settings:easeofaccess-keyboard",
        "zhanzhijian sticky keys filter keys",
    ),
    // ───────── 隐私和安全性 Privacy & security ─────────
    s(
        "隐私和安全性",
        "Privacy & security",
        "ms-settings:privacy",
        "yinsi anquan yinsi privacy security",
    ),
    s(
        "Windows 安全中心",
        "Windows Security",
        "windowsdefender:",
        "anquan zhongxin defender 杀毒 病毒 防护 windows security 安全",
    ),
    s(
        "病毒和威胁防护",
        "Virus & threat protection",
        "windowsdefender://threat",
        "bingdu weixie fanghu 杀毒 病毒 扫描 virus defender 排除项 exclusions",
    ),
    s(
        "设备加密 / BitLocker",
        "Device encryption",
        "ms-settings:deviceencryption",
        "shebei jiami bitlocker 加密",
    ),
    s(
        "BitLocker 驱动器加密",
        "BitLocker Drive Encryption",
        "control.exe /name Microsoft.BitLockerDriveEncryption",
        "bitlocker 驱动器加密",
    ),
    s(
        "查找我的设备",
        "Find my device",
        "ms-settings:findmydevice",
        "chazhao shebei find my device",
    ),
    s(
        "位置",
        "Location",
        "ms-settings:privacy-location",
        "weizhi wz location 定位",
    ),
    s(
        "摄像头隐私",
        "Camera privacy",
        "ms-settings:privacy-webcam",
        "shexiangtou yinsi webcam privacy 摄像头权限",
    ),
    s(
        "麦克风隐私",
        "Microphone privacy",
        "ms-settings:privacy-microphone",
        "maikefeng yinsi microphone privacy 麦克风权限",
    ),
    s(
        "诊断和反馈",
        "Diagnostics & feedback",
        "ms-settings:privacy-feedback",
        "zhenduan fankui diagnostics telemetry 遥测",
    ),
    s(
        "活动历史记录",
        "Activity history",
        "ms-settings:privacy-activityhistory",
        "huodong lishi activity history",
    ),
    s(
        "文件系统权限",
        "File system privacy",
        "ms-settings:privacy-broadfilesystemaccess",
        "wenjian xitong quanxian",
    ),
    s(
        "开发者选项",
        "For developers",
        "ms-settings:developers",
        "kaifazhe xuanxiang developer 开发人员 开发者模式 developer mode sudo",
    ),
    s(
        "安全和维护",
        "Security and Maintenance",
        "wscui.cpl",
        "anquan weihu maintenance 操作中心 action center",
    ),
    // ───────── Windows 更新 ─────────
    s(
        "Windows 更新",
        "Windows Update",
        "ms-settings:windowsupdate",
        "gengxin gx windows update 系统更新 检查更新 升级",
    ),
    s(
        "更新历史记录",
        "Update history",
        "ms-settings:windowsupdate-history",
        "gengxin lishi update history 卸载更新",
    ),
    s(
        "暂停更新 / 高级选项",
        "Update advanced options",
        "ms-settings:windowsupdate-options",
        "zanting gengxin pause 高级选项 active hours 使用时段",
    ),
    s(
        "传递优化",
        "Delivery Optimization",
        "ms-settings:delivery-optimization",
        "chuandi youhua delivery optimization",
    ),
    s(
        "Windows 预览体验计划",
        "Windows Insider Program",
        "ms-settings:windowsinsider",
        "yulan tiyan insider 预览体验",
    ),
    // ───────── 管理工具 / 控制台 ─────────
    s(
        "控制面板",
        "Control Panel",
        "control.exe",
        "kongzhi mianban kzmb control panel 控制板",
    ),
    s(
        "计算机管理",
        "Computer Management",
        "compmgmt.msc",
        "jisuanji guanli jsjgl computer management 管理",
    ),
    s(
        "磁盘管理",
        "Disk Management",
        "diskmgmt.msc",
        "cipan guanli cpgl disk management 分区 格式化 驱动器号 partition",
    ),
    s(
        "服务",
        "Services",
        "services.msc",
        "fuwu fw services 启动服务 停止服务",
    ),
    s(
        "任务计划程序",
        "Task Scheduler",
        "taskschd.msc",
        "renwu jihua chengxu rwjh task scheduler 定时任务 计划任务",
    ),
    s(
        "事件查看器",
        "Event Viewer",
        "eventvwr.msc",
        "shijian chakanqi event viewer 日志 log 错误日志",
    ),
    s(
        "注册表编辑器",
        "Registry Editor",
        "regedit.exe",
        "zhuceibiao regedit zcb 注册表",
    ),
    a(
        "本地组策略编辑器",
        "Local Group Policy Editor",
        "gpedit.msc",
        "zuzhengce gpedit group policy 组策略 本地策略",
    ),
    s(
        "本地安全策略",
        "Local Security Policy",
        "secpol.msc",
        "anquan celue secpol security policy 密码策略 账户锁定",
    ),
    s(
        "证书管理 (当前用户)",
        "Certificates",
        "certmgr.msc",
        "zhengshu certmgr certificates 证书",
    ),
    s(
        "共享文件夹",
        "Shared Folders",
        "fsmgmt.msc",
        "gongxiang wenjianjia shared folders 共享",
    ),
    s(
        "性能监视器",
        "Performance Monitor",
        "perfmon.msc",
        "xingneng jianshiqi perfmon performance monitor",
    ),
    s(
        "资源监视器",
        "Resource Monitor",
        "resmon.exe",
        "ziyuan jianshiqi resmon resource monitor 端口占用 网络占用",
    ),
    s(
        "系统配置 (msconfig)",
        "System Configuration",
        "msconfig.exe",
        "xitong peizhi msconfig 引导 安全模式 boot safe mode",
    ),
    s(
        "Windows 内存诊断",
        "Windows Memory Diagnostic",
        "mdsched.exe",
        "neicun zhenduan memory diagnostic 内存检测",
    ),
    s(
        "磁盘清理",
        "Disk Cleanup",
        "cleanmgr.exe",
        "cipan qingli cleanmgr disk cleanup 清理垃圾",
    ),
    s(
        "碎片整理和优化驱动器",
        "Optimize Drives",
        "dfrgui.exe",
        "suipian zhengli defrag optimize drives 优化驱动器 trim",
    ),
    s(
        "打印管理",
        "Print Management",
        "printmanagement.msc",
        "dayin guanli print management",
    ),
    s(
        "组件服务",
        "Component Services",
        "dcomcnfg.exe",
        "zujian fuwu dcom component services",
    ),
    s(
        "ODBC 数据源",
        "ODBC Data Sources",
        "odbcad32.exe",
        "odbc shujuyuan data sources",
    ),
    s(
        "Windows PowerShell ISE",
        "PowerShell ISE",
        "powershell_ise.exe",
        "powershell ise",
    ),
    s(
        "Windows 移动中心",
        "Windows Mobility Center",
        "mblctr.exe",
        "yidong zhongxin mobility center 亮度 笔记本",
    ),
    s(
        "远程桌面连接",
        "Remote Desktop Connection",
        "mstsc.exe",
        "yuancheng zhuomian lianjie mstsc rdp 远程连接",
    ),
    s(
        "Windows 版本信息",
        "About Windows (winver)",
        "winver.exe",
        "winver banben 版本号 windows version",
    ),
    s(
        "字符映射表",
        "Character Map",
        "charmap.exe",
        "zifu yingshe biao charmap 特殊符号",
    ),
    s(
        "截图工具",
        "Snipping Tool",
        "snippingtool.exe",
        "jietu gongju snipping tool 截图 screenshot",
    ),
    s(
        "放大镜 (程序)",
        "Magnifier app",
        "magnify.exe",
        "fangdajing magnify",
    ),
    s(
        "计算器",
        "Calculator",
        "calc.exe",
        "jisuanqi jsq calc calculator",
    ),
    s(
        "记事本",
        "Notepad",
        "notepad.exe",
        "jishiben jsb notepad 文本",
    ),
    s("画图", "Paint", "mspaint.exe", "huatu mspaint paint"),
    s("写字板", "WordPad", "write.exe", "xiezi ban wordpad write"),
    s(
        "步骤记录器",
        "Steps Recorder",
        "psr.exe",
        "buzhou jilu psr steps recorder",
    ),
    s(
        "Windows 传真和扫描",
        "Windows Fax and Scan",
        "wfs.exe",
        "chuanzhen saomiao fax scan",
    ),
    s(
        "系统还原",
        "System Restore",
        "rstrui.exe",
        "xitong huanyuan rstrui restore",
    ),
    s(
        "显示切换 (投影)",
        "Display switch",
        "DisplaySwitch.exe",
        "xianshi qiehuan 投影 projector 复制 扩展 第二屏幕 duplicate extend",
    ),
    s(
        "颜色管理",
        "Color Management",
        "colorcpl.exe",
        "yanse guanli color management 显示器校色",
    ),
    s(
        "ClearType 文本调谐器",
        "ClearType Text Tuner",
        "cttune.exe",
        "cleartype 文字 模糊",
    ),
    s(
        "DPI 缩放 (显示)",
        "DPI scaling",
        "ms-settings:display",
        "dpi suofang scaling 缩放 125% 150%",
    ),
    s(
        "启动文件夹 (开机自启)",
        "Startup folder",
        "shell:startup",
        "qidong wenjianjia startup folder 开机启动文件夹 shell:startup",
    ),
    s(
        "所有用户启动文件夹",
        "Common startup folder",
        "shell:common startup",
        "suoyou yonghu qidong common startup",
    ),
    s(
        "AppData 文件夹",
        "AppData folder",
        "shell:appdata",
        "appdata roaming 应用数据",
    ),
    s(
        "Local AppData 文件夹",
        "Local AppData folder",
        "shell:local appdata",
        "localappdata 应用数据",
    ),
    s(
        "下载文件夹",
        "Downloads folder",
        "shell:downloads",
        "xiazai downloads 下载",
    ),
    s(
        "文档文件夹",
        "Documents folder",
        "shell:personal",
        "wendang documents 文档",
    ),
    s(
        "桌面文件夹",
        "Desktop folder",
        "shell:desktop",
        "zhuomian desktop 桌面",
    ),
    s(
        "回收站",
        "Recycle Bin",
        "shell:RecycleBinFolder",
        "huishouzhan hsz recycle bin 回收站",
    ),
    s(
        "此电脑",
        "This PC",
        "shell:MyComputerFolder",
        "ci diannao 我的电脑 this pc computer",
    ),
    s(
        "网络位置",
        "Network locations",
        "shell:NetworkPlacesFolder",
        "wangluo weizhi network places",
    ),
    s(
        "临时文件夹 (%TEMP%)",
        "Temp folder",
        "%TEMP%",
        "linshi wenjianjia temp tmp 临时文件",
    ),
    s(
        "发送到文件夹",
        "SendTo folder",
        "shell:sendto",
        "fasong dao sendto",
    ),
    s(
        "最近使用的文件",
        "Recent files",
        "shell:recent",
        "zuijin wenjian recent 最近",
    ),
    s(
        "所有任务 (上帝模式)",
        "All Tasks (God Mode)",
        "explorer.exe shell:::{ED7BA470-8E54-465E-825C-99712043E01C}",
        "shangdi moshi god mode all tasks 所有任务 全部设置",
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_sane() {
        assert!(SETTINGS.len() > 150);
        for d in SETTINGS {
            assert!(!d.zh.is_empty() && !d.en.is_empty() && !d.target.is_empty());
        }
    }

    #[test]
    fn required_entries_exist() {
        for needle in [
            "环境变量",
            "Windows 防火墙",
            "高级安全 Windows 防火墙",
            "设备管理器",
            "Windows 更新",
        ] {
            assert!(SETTINGS.iter().any(|d| d.zh == needle), "{needle}");
        }
    }
}
