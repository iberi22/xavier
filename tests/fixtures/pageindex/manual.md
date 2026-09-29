# Aurora Router Manual

The Aurora AR-500 is a dual-band wireless router for small offices.
This manual covers installation, configuration, security and maintenance.

# 1. Installation

## 1.1 Unpacking

The box contains the router, a 12V power adapter, one Ethernet cable and a wall-mount bracket.
Keep the original packaging for warranty returns.
Check that the serial number on the base matches the number on the box label.

## 1.2 Placing the router

Place the router in a central, elevated position away from metal shelves and microwave ovens.
Concrete walls reduce the wireless range by roughly half.
Leave at least ten centimeters of free space around the vents to avoid overheating.

## 1.3 Connecting the cables

Plug the Ethernet cable into the yellow WAN port and connect the other end to your modem.
Attach the power adapter and wait until the status light turns solid green.
The first boot takes about ninety seconds.
Never connect a device to the WAN port other than the modem.

# 2. Configuration

## 2.1 Web administration panel

Open a browser and visit the address 192.168.50.1 to reach the administration panel.
The default administrator user is admin and the initial password is printed on the base label.
You are forced to choose a new password on first login.

## 2.2 Wireless networks

The router broadcasts a 2.4 GHz network for legacy devices and a 5 GHz network for fast connections.
Each network can have its own name and passphrase.
Use channel 1, 6 or 11 on the 2.4 GHz band to avoid overlapping neighbours.
Channel width on the 5 GHz band can be set to 80 MHz for maximum throughput.

## 2.3 Guest network

A guest network isolates visitors from your office computers.
Guests can reach the internet but not the local printers or file servers.
The guest passphrase can expire automatically after a chosen number of hours.

## 2.4 Port forwarding

To expose an internal server, open the Forwarding page and add a rule with the external port,
the internal address and the protocol.
Rules take effect immediately without a reboot.
Only forward ports that you actually need because every open port increases exposure.

# 3. Security

## 3.1 Firmware updates

Firmware updates fix vulnerabilities and are published every quarter.
Enable automatic updates in the Maintenance page so the router installs them at night.
Do not power off the router while the update light blinks amber, or the device may become unusable.

## 3.2 Firewall

The built-in firewall drops all unsolicited inbound traffic by default.
Advanced users can create custom rules that allow or block traffic by address, port or schedule.
Logging of blocked connections is disabled by default to save flash memory.

## 3.3 Parental controls

Parental controls let you block categories of websites and limit internet time per device.
Assign each child device to a profile and set the allowed hours.
Blocked requests show a notice page explaining the restriction.

## 3.4 VPN server

The router can act as a WireGuard VPN server so that remote staff reach the office network.
Generate a key pair for each user and download the configuration file or scan the QR code.
The VPN uses UDP port 51820 by default.

# 4. Maintenance

## 4.1 Factory reset

To restore factory settings, hold the recessed reset button for ten seconds with the router powered on.
All passwords, wireless names and forwarding rules are erased.
Export a configuration backup beforehand if you want to restore your settings later.

## 4.2 Troubleshooting

If the internet light is red, check the cable between the router and the modem and restart both devices.
If wireless speed is slow, change the channel and move the router away from interference.
If you forget the administrator password, a factory reset is the only recovery method.

## 4.3 Warranty and support

The AR-500 carries a three year limited warranty against manufacturing defects.
Damage caused by liquids, lightning or unauthorized repairs is not covered.
Contact support with the serial number and a description of the problem.
