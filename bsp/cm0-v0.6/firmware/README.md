# M5Stack boot-screen firmware

`start-m5stack-bootscreen.elf` is the retired opaque VideoCore firmware distributed
by M5Stack's CardputerZero image builder. It retains the Raspberry Pi `start_x`
camera feature set and adds early ST7789 splash rendering from
`/boot/firmware/splash.bmp`. M5Stack installs this embedded `start_x` variant as
`/boot/firmware/start.elf` without setting `start_x=1`, so it remains paired
with `/boot/firmware/fixup.dat` exactly as shipped in the working official
image.

- Source repository: `https://github.com/CardputerZero/pi-gen`
- Source commit: `554544921c1659f39bf296b7986715fdeac898c8`
- Source path: `stage2/05-cardputerzero/files/start.elf`
- SHA256: `d1639763fa6714e2cd4544fb45b9d5e5d54e949eaa11d7e7057651b6d4d51efd`
- Paired `fixup.dat` SHA256: `b2d19b8c300b5a4ddbd0fcff3a0f7de61a171046269d8724e74f616058417d4b`
- Embedded branch: `m5stack_bootscreen`
- Embedded variant: `start_x`
- Embedded upstream version: `85bf5729aa4fa558b105936b0841241dc4b9ee64 (tainted)`

The artifact is retained only as provenance and is not packaged. V0.6 testing
showed that it ignores `gpu_mem_512=64` and forces a 256 MB GPU split, leaving
Linux with only about 227 MiB. Image construction and the final rootfs gate now
reject this hash. Product and recovery images use the `raspi-firmware`
`start_x.elf`/`fixup_x.dat` pair selected by `start_x=1`; Linux renders the
product splash after the LCD framebuffer appears.
