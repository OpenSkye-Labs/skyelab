//! The workspace's window-wide actions.

use gpui::actions;

actions!(
    skyelab,
    [
        ToggleSidebar,
        ToggleBottomPanel,
        NewTerminal,
        OpenSettings,
        OpenBrowser,
        AddProject,
        NewThread,
        OpenDevices,
        ShowAllProjects,
        GoBack,
        GoForward,
        OpenOnboarding,
        OpenUsage,
        OpenHome,
        OpenBots
    ]
);
