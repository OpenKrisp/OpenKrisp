on run
    try
        display dialog "Remove OpenKrisp and its two audio devices? Close calling and audio apps first. Your saved settings will be kept. You will need an administrator password and a restart afterward." with title "Uninstall OpenKrisp" buttons {"Cancel", "Uninstall"} default button "Cancel" cancel button "Cancel" with icon caution
        try
            tell application id "app.openkrisp.desktop" to quit
        end try
        set cleanup to (POSIX path of (path to me)) & "Contents/Resources/uninstall.command"
        do shell script "/bin/bash " & quoted form of cleanup with administrator privileges
        display dialog "OpenKrisp and its audio drivers have been removed. Restart your Mac to unload the drivers, then select your physical microphone and speakers in calling apps." with title "OpenKrisp removed" buttons {"OK"} default button "OK"
    on error messageText number errorNumber
        if errorNumber is not -128 then
            display alert "OpenKrisp could not be fully removed" message messageText & " Quit OpenKrisp and audio apps, then run this uninstaller again." as critical
        end if
    end try
end run
