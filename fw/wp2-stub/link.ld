MEMORY { FLASH (rx) : ORIGIN = 0x1025A080, LENGTH = 0x6000 }

PROVIDE(stock_heap_init = 0x10146AA5);
ENTRY(wp2_boot)

SECTIONS
{
  .header : { KEEP(*(.header)) } > FLASH
  .text : { *(.text .text.*) } > FLASH
  .rodata : { *(.rodata .rodata.*) } > FLASH
  .data : { *(.data .data.*) } > FLASH
  .bss : { *(.bss .bss.* COMMON) } > FLASH
  /DISCARD/ : { *(.ARM.exidx .ARM.exidx.* .ARM.extab .ARM.extab.*) }
}

ASSERT(ADDR(.header) == 0x1025A080, "stub header must come first");
ASSERT(SIZEOF(.data) + SIZEOF(.bss) == 0, "stub must not have writable statics");
