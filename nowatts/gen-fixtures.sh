#!/usr/bin/env bash
# Fixture sysfs trees for nowatts hermetic tests (FR-004).
# Values mirror live observations from the target machine (2026-10-19) and
# generic-hardware variants (owner directive 2026-10-19).
set -euo pipefail
cd "$(dirname "$0")"

F=fixtures

# ---------- helpers ----------
attr() { # attr <path> <name> <value>
    mkdir -p "$(dirname "$1/$2")"
    printf '%s\n' "$3" > "$1/$2"
}

port_common() { # port_common <class/typec/portN dir>
    attr "$1" power_role "source [sink]"
    attr "$1" usb_power_delivery_revision "2.0"
    attr "$1" usb_capability "usb2 [usb3]"
    attr "$1" vconn_source "no"
}

add_location() { # add_location <port dir>   (top/upper/left as observed live)
    attr "$1/physical_location" panel top
    attr "$1/physical_location" vertical_position upper
    attr "$1/physical_location" horizontal_position left
}

add_partner() { # add_partner <class/typec dir> <portN>
    mkdir -p "$1/$2-partner"
    printf 'yes\n' > "$1/$2-partner/supports_usb_power_delivery"
    printf '2.0\n' > "$1/$2-partner/usb_power_delivery_revision"
}

add_alt_modes() { # add_alt_modes <class/typec dir> <portN>  (incl. active ff01 DP)
    # Live layout: alt-mode dirs sit INSIDE the port dir.
    local p="$1/$2"
    attr "$p/$2.0" mode "1"; attr "$p/$2.0" svid "17ef"
    attr "$p/$2.1" mode "1"; attr "$p/$2.1" svid "8087"
    attr "$p/$2.2" mode "1"; attr "$p/$2.2" svid "ff01"
    attr "$p/$2.2/mode1" active yes
    attr "$p/$2.2/mode1" vdo 0x001c1c43
}

# UCSI-style psys as observed live (names carry the trailing index).
psy_ucsi_idle() { # <class/power_supply dir>
    local d="$1/ucsi-source-psy-USBC000:001"
    attr "$d" type USB
    attr "$d" online 0
    attr "$d" status "Not charging"
    attr "$d" usb_type "[C] PD PD_PPS"
    attr "$d" voltage_max 0
    attr "$d" current_max 0
    attr "$d" voltage_now 0
    attr "$d" current_now 0
    attr "$d" charge_type "N/A"
}
psy_ucsi_charging() { # <dir> [vmax imax vnow inow status online]
    local d="$1/ucsi-source-psy-USBC000:002"
    local vmax=${2:-20000000} imax=${3:-3000000} vnow=${4:-20000000} inow=${5:-3000000} status=${6:-Charging} online=${7:-1}
    attr "$d" type USB
    attr "$d" online "$online"
    attr "$d" status "$status"
    attr "$d" usb_type "C [PD] PD_PPS"
    attr "$d" voltage_max "$vmax"
    attr "$d" current_max "$imax"
    attr "$d" voltage_now "$vnow"
    attr "$d" current_now "$inow"
    attr "$d" charge_type Standard
}
battery_live() { # battery_live <class/power_supply dir> <name> <status> <power_now> <enow> <efull>
    local d="$1/$2"
    attr "$d" type Battery
    attr "$d" status "$3"
    attr "$d" power_now "$4"
    attr "$d" energy_now "$5"
    attr "$d" energy_full "$6"
    attr "$d" voltage_now 13334000
}
mains() { # mains <dir> <name> <online>
    local d="$1/$2"
    attr "$d" type Mains
    attr "$d" online "$3"
}

usb_dev() { # usb_dev <bus/usb/devices dir> <name> <speed> <product> <manufacturer> [idVendor]
    local d="$1/$2"
    mkdir -p "$d"
    attr "$d" speed "$3"
    [ -n "$4" ] && attr "$d" product "$4"
    [ -n "$5" ] && attr "$d" manufacturer "$5"
    [ -n "${6:-}" ] && attr "$d" idVendor "$6"
}

tb_tree() { # tb_tree <sysfs root>  — host router 0-0 + Plugable TB3 dock 0-1 (live values)
    local router="$1/devices/pci0000:00/0000:00:0d.2/domain0/0-0"
    mkdir -p "$router/usb4_port1"
    attr "$router" device_name Gen16
    attr "$router" vendor_name Intel
    attr "$router" generation 4
    attr "$router" authorized 1
    attr "$router/0-1" device_name "TBT3-UDV Docking Station"
    attr "$router/0-1" vendor_name Plugable
    attr "$router/0-1" generation 3
    attr "$router/0-1" authorized 1
}

add_tb_link() { # add_tb_link <port dir> <sysfs root>  — resolving usb4_port1 link
    ln -sfn "../../../devices/pci0000:00/0000:00:0d.2/domain0/0-0/usb4_port1" "$1/usb4_port1"
}

add_hub_links() { # add_hub_links <port dir>  — dangling name-only links (live names)
    ln -sfn "../../../devices/pci0000:00/0000:00:0d.0/usb2/2-0:1.0/usb2-port1" "$1/usb2-port1"
    ln -sfn "../../../devices/pci0000:00/0000:00:14.0/usb3/3-0:1.0/usb3-port2" "$1/usb3-port2"
}

drm_conn() { # drm_conn <class/drm dir> <connector (e.g. 0-DP-1)> <status>
    mkdir -p "$1/card$2"
    attr "$1/card$2" status "$3"
    attr "$1/card$2" enabled "$([ "$3" = connected ] && echo enabled || echo disabled)"
}

# ---------- case 1: charger-charging (live mirror) ----------
c="$F/charger-charging/sysfs"
mkdir -p "$c/class/typec"
port_common "$c/class/typec/port0"; add_location "$c/class/typec/port0"; attr "$c/class/typec/port0" data_role "host [device]"
port_common "$c/class/typec/port1"; add_location "$c/class/typec/port1"; attr "$c/class/typec/port1" data_role "[host] device"
add_partner "$c/class/typec" port1
add_alt_modes "$c/class/typec" port1   # live UCSI quirk: active ff01 flag even with no link
psy_ucsi_idle "$c/class/power_supply"
psy_ucsi_charging "$c/class/power_supply"
battery_live "$c/class/power_supply" BAT0 Charging 23974000 51920000 57010000
mains "$c/class/power_supply" AC 1

# ---------- case 2: charging-no-contract ----------
c="$F/charging-no-contract/sysfs"
mkdir -p "$c/class/typec"
port_common "$c/class/typec/port0"; add_location "$c/class/typec/port0"; attr "$c/class/typec/port0" data_role "host [device]"
port_common "$c/class/typec/port1"; add_location "$c/class/typec/port1"; attr "$c/class/typec/port1" data_role "[host] device"
add_partner "$c/class/typec" port1
psy_ucsi_idle "$c/class/power_supply"
psy_ucsi_charging "$c/class/power_supply" 0 0 5000000 2000000
battery_live "$c/class/power_supply" BAT0 Charging 23974000 51920000 57010000
mains "$c/class/power_supply" AC 1

# ---------- case 3: idle-no-partner ----------
c="$F/idle-no-partner/sysfs"
mkdir -p "$c/class/typec"
port_common "$c/class/typec/port0"; add_location "$c/class/typec/port0"; attr "$c/class/typec/port0" data_role "host [device]"
port_common "$c/class/typec/port1"; add_location "$c/class/typec/port1"; attr "$c/class/typec/port1" data_role "[host] device"
psy_ucsi_idle "$c/class/power_supply"
psy_ucsi_charging "$c/class/power_supply" 0 0 0 0 "Not charging" 0
battery_live "$c/class/power_supply" BAT0 Discharging -8500000 30000000 57010000
mains "$c/class/power_supply" AC 1

# ---------- case 4: no-typec-degraded ----------
c="$F/no-typec-degraded/sysfs"
psy_ucsi_idle "$c/class/power_supply"
psy_ucsi_charging "$c/class/power_supply"
battery_live "$c/class/power_supply" BAT0 Charging 23974000 51920000 57010000
mains "$c/class/power_supply" AC 1

# ---------- case 5: weak-charger (AC drain warning wins) ----------
c="$F/weak-charger/sysfs"
mkdir -p "$c/class/typec"
port_common "$c/class/typec/port0"; add_location "$c/class/typec/port0"; attr "$c/class/typec/port0" data_role "host [device]"
port_common "$c/class/typec/port1"; add_location "$c/class/typec/port1"; attr "$c/class/typec/port1" data_role "[host] device"
add_partner "$c/class/typec" port1
psy_ucsi_idle "$c/class/power_supply"
psy_ucsi_charging "$c/class/power_supply" 5000000 3000000 5000000 2500000
battery_live "$c/class/power_supply" BAT0 Discharging -3000000 51920000 57010000
mains "$c/class/power_supply" AC 1

# ---------- case 6: weak-charger-slow-only (slow-charge warning only) ----------
c="$F/weak-charger-slow-only/sysfs"
mkdir -p "$c/class/typec"
port_common "$c/class/typec/port0"; add_location "$c/class/typec/port0"; attr "$c/class/typec/port0" data_role "host [device]"
port_common "$c/class/typec/port1"; add_location "$c/class/typec/port1"; attr "$c/class/typec/port1" data_role "[host] device"
add_partner "$c/class/typec" port1
psy_ucsi_idle "$c/class/power_supply"
psy_ucsi_charging "$c/class/power_supply" 5000000 3000000 5000000 2500000
battery_live "$c/class/power_supply" BAT0 Charging 19200000 51920000 57010000
mains "$c/class/power_supply" AC 1

# ---------- case 7: usb-device-attached ----------
c="$F/usb-device-attached/sysfs"
mkdir -p "$c/class/typec"
port_common "$c/class/typec/port0"; add_location "$c/class/typec/port0"; attr "$c/class/typec/port0" data_role "host [device]"
add_hub_links "$c/class/typec/port0"
port_common "$c/class/typec/port1"; add_location "$c/class/typec/port1"; attr "$c/class/typec/port1" data_role "[host] device"
u="$c/bus/usb/devices"
usb_dev "$u" usb1 480 "xHCI Host Controller" "Linux Foundation" 1d6b
usb_dev "$u" usb2 20000 "xHCI Host Controller" "Linux Foundation" 1d6b
usb_dev "$u" usb3 480 "xHCI Host Controller" "Linux Foundation" 1d6b
usb_dev "$u" usb4 20000 "xHCI Host Controller" "Linux Foundation" 1d6b
usb_dev "$u" 2-1 5000 "SanDisk Extreme SSD" "SanDisk" 0781
usb_dev "$u" 2-1.4 5000 "USB3 Hub" "Generic" 1a40
mkdir -p "$u/2-1:1.0"   # interface: no speed file, excluded by ':' filter
usb_dev "$u" 3-4 480 "Integrated Camera" "Chicony" 04f2
usb_dev "$u" 3-5 480 "USB2.0 Hub" "Genesys" 05e3
usb_dev "$u" 3-5.4 12 "Fingerprint Reader" "Goodix" 27c6
psy_ucsi_idle "$c/class/power_supply"
psy_ucsi_charging "$c/class/power_supply" 0 0 0 0 "Not charging" 0
battery_live "$c/class/power_supply" BAT0 Discharging -8500000 30000000 57010000
mains "$c/class/power_supply" AC 1

# ---------- case 8: display-connected ----------
c="$F/display-connected/sysfs"
mkdir -p "$c/class/typec"
port_common "$c/class/typec/port0"; add_location "$c/class/typec/port0"; attr "$c/class/typec/port0" data_role "host [device]"
add_tb_link "$c/class/typec/port0" "$c"
add_alt_modes "$c/class/typec" port0
port_common "$c/class/typec/port1"; add_location "$c/class/typec/port1"; attr "$c/class/typec/port1" data_role "[host] device"
add_alt_modes "$c/class/typec" port1   # link-less port: flag present but must stay gated off
tb_tree "$c"
d="$c/class/drm"
drm_conn "$d" 0-DP-1 connected
drm_conn "$d" 0-DP-2 disconnected
drm_conn "$d" 0-eDP-1 connected
drm_conn "$d" 0-HDMI-A-1 disconnected
usb_dev "$c/bus/usb/devices" 5-2 12 "Plugable TBT3-UDV Dock Audio" "Texas Instruments" 2230
psy_ucsi_idle "$c/class/power_supply"
psy_ucsi_charging "$c/class/power_supply" 0 0 0 0 "Not charging" 0
battery_live "$c/class/power_supply" BAT0 Charging 23974000 51920000 57010000
mains "$c/class/power_supply" AC 1

# ---------- case 9: battery-full ----------
c="$F/battery-full/sysfs"
mkdir -p "$c/class/typec"
port_common "$c/class/typec/port0"; add_location "$c/class/typec/port0"; attr "$c/class/typec/port0" data_role "host [device]"
port_common "$c/class/typec/port1"; add_location "$c/class/typec/port1"; attr "$c/class/typec/port1" data_role "[host] device"
add_partner "$c/class/typec" port1
psy_ucsi_idle "$c/class/power_supply"
psy_ucsi_charging "$c/class/power_supply" 20000000 3000000 0 0 "Not charging"
battery_live "$c/class/power_supply" BAT0 Full 0 56800000 57010000
mains "$c/class/power_supply" AC 1

# ---------- case 10: supplying ----------
c="$F/supplying/sysfs"
mkdir -p "$c/class/typec"
port_common "$c/class/typec/port0"; add_location "$c/class/typec/port0"
attr "$c/class/typec/port0" power_role "[source] sink"
attr "$c/class/typec/port0" data_role "[host] device"
port_common "$c/class/typec/port1"; add_location "$c/class/typec/port1"; attr "$c/class/typec/port1" data_role "[host] device"
add_partner "$c/class/typec" port0
psy_ucsi_idle "$c/class/power_supply"
psy_ucsi_charging "$c/class/power_supply" 0 0 0 0 "Not charging" 0
battery_live "$c/class/power_supply" BAT0 Discharging -1200000 30000000 57010000
mains "$c/class/power_supply" AC 0

# ---------- case 11: generic-single-port (non-UCSI hardware) ----------
c="$F/generic-single-port/sysfs"
mkdir -p "$c/class/typec"
port_common "$c/class/typec/port0"   # no physical_location on this hardware
attr "$c/class/typec/port0" data_role "host [device]"
add_partner "$c/class/typec" port0
d="$c/class/power_supply/tcpm-source-psy-3-0025"
attr "$d" type USB
attr "$d" online 1
attr "$d" status Charging
attr "$d" usb_type "[C] PD"
attr "$d" voltage_max 20000000
attr "$d" current_max 2250000
attr "$d" voltage_now 20000000
attr "$d" current_now 1500000
battery_live "$c/class/power_supply" BAT1 Charging 10500000 42100000 64800000
mains "$c/class/power_supply" ADP1 1

# ---------- case 12: multi-battery ----------
c="$F/multi-battery/sysfs"
mkdir -p "$c/class/typec"
port_common "$c/class/typec/port0"; add_location "$c/class/typec/port0"; attr "$c/class/typec/port0" data_role "host [device]"
port_common "$c/class/typec/port1"; add_location "$c/class/typec/port1"; attr "$c/class/typec/port1" data_role "[host] device"
psy_ucsi_idle "$c/class/power_supply"
psy_ucsi_charging "$c/class/power_supply" 0 0 0 0 "Not charging" 0
battery_live "$c/class/power_supply" BAT0 Charging 10000000 30000000 50000000
battery_live "$c/class/power_supply" BAT1 Discharging -2000000 2000000 2000000
mains "$c/class/power_supply" AC 1

# ---------- case 13: psy-unmapped ----------
c="$F/psy-unmapped/sysfs"
mkdir -p "$c/class/typec"
port_common "$c/class/typec/port0"; add_location "$c/class/typec/port0"; attr "$c/class/typec/port0" data_role "host [device]"
port_common "$c/class/typec/port1"; add_location "$c/class/typec/port1"; attr "$c/class/typec/port1" data_role "[host] device"
add_partner "$c/class/typec" port0
d="$c/class/power_supply/generic-usb-psy"   # no trailing index → name heuristic n/a
attr "$d" type USB
attr "$d" online 1
attr "$d" status Charging
attr "$d" usb_type "[C] PD"
attr "$d" voltage_max 20000000
attr "$d" current_max 3000000
attr "$d" voltage_now 20000000
attr "$d" current_now 3000000
battery_live "$c/class/power_supply" BAT0 Charging 8000000 40000000 57010000
mains "$c/class/power_supply" AC 1

echo "fixtures written:"
ls "$F"
