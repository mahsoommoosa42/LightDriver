# First BIOS curve capture test

This procedure targets the reviewed GIGABYTE X570 AORUS ELITE WIFI/F41c profile. It collects evidence for firmware decoding. The debugger cannot write firmware or restore fan settings; percentage and restore actions are offline previews.

## Before rebooting

1. Open the refactored `lightdriver-fan-debug.exe` with **Run as administrator**, accepting normal Windows UAC. If you cancel UAC, repeat the launch. No protection setting needs changing.
2. Close other software that controls motherboard fans for the capture experiment. Open **Snapshots & comparison**, click **Read firmware** and confirm a current recognized snapshot plus a verified initial backup. Stop if the read/backup fails and report the displayed message.
3. In the snapshot-path field, enter a new file in an existing private folder, such as the project's `.data/before-bios.private.json`, and click **Save current snapshot**. Use an absolute path if the launcher working directory is uncertain. Saves refuse overwrite: keep existing backups and choose a new filename instead.
4. Optional: **Start RPM monitor** records live tachometer readings through the separately installed official PawnIO driver and pinned LpcIO module. Note which fans are physically connected; the initial channel/header map is still provisional. A missing tach signal does not establish that a fan is stopped. Firmware capture itself does not need PawnIO.

## In BIOS

1. Enter BIOS and open **Smart Fan 5**. Select only a header physically confirmed to feed case fans: **SYS_FAN1** if that is the connected header, otherwise **SYS_FAN2**. Do not use CPU, pump or chipset headers for this experiment.
2. Photograph the original header settings, including its mode, graph, temperature input, PWM/DC/Auto mode, interval and Fan Stop.
3. Change only **Fan Speed Control** to **Manual** and set the following temporary test curve. Distinct points help identify the stored encoding:

| Temperature | Fan setting |
|---|---:|
| 20°C | 50% |
| 30°C | 60% |
| 40°C | 75% |
| 50°C | 90% |
| 60°C | 100% |

Keep temperature input, control mode, interval and Fan Stop unchanged. Photograph the final graph and all actual values; if BIOS rounds values or presents a different point layout, record what it accepts rather than altering unrelated options. Do not apply to other headers, load optimized defaults or update firmware during this comparison.

4. Save the BIOS changes and return to Windows.

## After rebooting

1. Reopen the debugger as administrator, click **Read firmware** and save a new **after-bios.private.json** in the same private folder.
2. **Important:** this new session's automatic Initial backup contains the new curve. Retain and use the explicit **before-bios** file when comparing with the original state.
3. To compare in the UI, enter the before file's path, click **Load baseline from path**, then **Read firmware** again. Inspect the changed fields. Keep both complete private files for local analysis.
4. Record the selected header, exact displayed BIOS points and whether the case fans changed speed. Optional live RPM can document response but does not independently verify wiring or temperature-dependent regulation.
5. After the capture, restore your photographed original settings through BIOS. The previous observations used Normal mode, but follow your actual before photograph if different. App restore buttons cannot perform this action yet.
6. Return with the local before/after paths and BIOS photographs/actual settings. Do not publish raw `.private.json` files to GitHub; they are ignored by the repository.

Completion for this experiment means a confirmed physical case header, exact BIOS settings and valid before/after captures. We then compare all stored bytes, including unknown fields, and check our decoding. A BIOS-made curve does not prove Windows write permission, app-created curve persistence or tested hardware restoration.
