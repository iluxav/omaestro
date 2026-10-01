# system-events

What the machine is doing, as notifications and journal lines.

| Event | What happens |
|---|---|
| wake from sleep | "Welcome back" with the time |
| a USB device plugged in | a notification naming it |
| battery at 15% or less while discharging | "find a charger" |
| network change | a journal line with nmcli's words |
| going to sleep | a journal line |

Sources: logind over D-Bus, udev, sysfs (polled every 30 s), NetworkManager.
Each runs only while this plugin listens to it.

## Install

```sh
om plugin add system-events
```

Options go in `~/.config/omaestro/rules.d/system-events.lua`:

```lua
om.use("system-events").setup({ battery_low = 20, usb = false })
```

## Options

- `wake`, `usb`, `network`, `sleep_log`: `true` or `false`.
- `battery_low`: the percent (`false`: off).
