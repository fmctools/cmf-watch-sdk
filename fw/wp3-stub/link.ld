/* the end of the stock jx402_08 1.0.1.77 app.bin (0x2BD280 bytes) */
ENTRY(wp3_wf_boot)
MEMORY { FLASH (rx) : ORIGIN = 0x102BD280, LENGTH = 0x5000 }

SECTIONS
{
  .header : { KEEP(*(.header)) } > FLASH
  .text : { *(.text .text.*) } > FLASH
  .rodata : { *(.rodata .rodata.*) } > FLASH
  .data : { *(.data .data.*) } > FLASH
  .bss : { *(.bss .bss.* COMMON) } > FLASH
  /DISCARD/ : { *(.ARM.exidx .ARM.exidx.* .ARM.extab .ARM.extab.*) }
}

ASSERT(ADDR(.header) == 0x102BD280, "stub header must come first");
ASSERT(SIZEOF(.data) + SIZEOF(.bss) == 0, "stub must not have writable statics");
