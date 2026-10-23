# CI fixtures only. Elevated runners may default new SQLite sidecars to Administrators.
# Select the current account as default owner for this process and its children;
# do not change privileges, existing ownership, ACLs, or product admission rules.
Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Security.Principal;
public static class PulseNativeFixtureOwner {
    [DllImport("advapi32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool SetTokenInformation(IntPtr token, int kind, ref IntPtr owner, int length);
    public static void UseCurrentAccount() {
        using (var identity = WindowsIdentity.GetCurrent(TokenAccessLevels.Query | TokenAccessLevels.AdjustDefault)) {
            var bytes = new byte[identity.User.BinaryLength];
            identity.User.GetBinaryForm(bytes, 0);
            var pin = GCHandle.Alloc(bytes, GCHandleType.Pinned);
            try {
                var owner = pin.AddrOfPinnedObject();
                // TOKEN_OWNER contains one SID pointer. Only the token's current user is selected.
                if (!SetTokenInformation(identity.Token, 4, ref owner, IntPtr.Size))
                    throw new Win32Exception(Marshal.GetLastWin32Error());
            } finally { pin.Free(); }
        }
    }
}
'@
[PulseNativeFixtureOwner]::UseCurrentAccount()
