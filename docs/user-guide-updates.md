# User guide: updates

Once a day at most, when SkyeLab starts (and every hour after that it checks whether a day has passed), it asks
GitHub for the latest release of SkyeLab. When that release is newer than yours, a small pill appears in the
sidebar's bottom row, beside Refresh: **Update available**, then **Downloading… 42%**, **Ready to
restart** or **Update failed** (with **Retry**). A narrow sidebar shortens the label, and a very narrow one shows only
its icon (hover it for the label). Click the pill for the details and the actions: **Download and install** and
**Skip this version**, which hides it until a newer one comes out.
**Check for Updates** (the palette, or the SkyeLab menu on macOS) and **Check now** in **Settings → About** check
right away, skipped version included. Before the first release is published, they say **No release published
yet**. The requests carry nothing about you. **Settings → General → Check for
updates** turns the daily check off.

**Settings → General → Update channel** picks what is offered. **Stable** (the default) offers published releases
only. **Nightly** also offers the nightly builds of the next version (`0.1.8-nightly.20261128`), published when the
maintainers run one, and moves you on to the next stable release once it is out, since `0.1.8` is newer than any
`0.1.8-nightly`. A nightly build starts on Nightly; picking Stable there waits for the next stable release rather
than going back. When the offer is a nightly, the pill says **Nightly available** and its details name the nightly.
Both channels download and verify the same way.

**Download and install** downloads the release's file for your computer (with its progress in the pill) and checks
it against the `SHA256SUMS` list published with the release. On macOS it also opens the disk image, copies the new
app next to yours; it must be SkyeLab (the same bundle identifier) and, when your SkyeLab is signed with a Developer ID, signed by the
same developer and accepted by Gatekeeper. On Windows a signed SkyeLab runs only an installer validly signed by the
same publisher. Nothing you run changes yet. When it says **SkyeLab x.y.z is ready**, click **Restart to
update**: SkyeLab replaces itself and opens again.

- **macOS:** the `.app` is swapped in place (a failed swap puts the old one back).
- **Windows:** the installer runs silently once SkyeLab has quit, then starts it again. If the installer fails, your
  old SkyeLab opens and the pill says why.
- **Linux AppImage:** the AppImage file is replaced in place. **.deb:** the package is saved to your Downloads folder
  and **Show in folder** opens it there; install it with your package manager.

If anything fails, the pill says so, and its details say why and offer **Try again** and **Release page**; your installed SkyeLab is left
as it was. A copy that can't update itself (built from source) shows **Download**, which opens the release page.
