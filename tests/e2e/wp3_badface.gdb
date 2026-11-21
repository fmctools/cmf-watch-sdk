# Watch 3 Pro boot guard e2e (tests/test_wfguard_e2e.py): models a bad built-in face with breakpoints on the stock
# jx402_08 1.0.1.77. Run after `target remote` with $udf (a `udf` in the stub) and $flag (its flag path) set.
set $faulted = 0
set $confirm = 0
# the default list built while no saved list exists (until the fault): make entry 1 (276) active, as if the user
# had picked it, before the builder saves it (0x100D7BA2: bl WfmPara_Save, r4 = para)
break *0x100d7ba2 if $faulted == 0
commands
  silent
  set *(unsigned char *)($r4 + 5) = 1
  set *(unsigned char *)($r4 + 6) = *(unsigned char *)($r4 + 0xC + 16 + 4)
  set *(unsigned *)($r4 + 8) = *(unsigned *)($r4 + 0xC + 16)
  printf "ACTIVE set to %u\n", *(unsigned *)($r4 + 8)
  continue
end
# models a bad face 276: fault (udf in wp3-stub) once WFManager_Init picked it, after the stub's hook ran
break *0x100d6e42 if *(unsigned *)0x38026cb4 == 276
commands
  silent
  set $faulted = 1
  printf "BADFACE fault id=%u\n", *(unsigned *)0x38026cb4
  set $pc = $udf
  continue
end
# the stub's confirm timer: fs_unlink (0x10116638) of the flag, then lv_timer_del (0x10148F04); once both ran,
# reset the watch (QEMU system_reset) like a watchdog or AT REBOOT, which skip the stock's storage flush
break *0x10116638 if $r0 == $flag
commands
  silent
  set $confirm = 1
  printf "CONFIRM unlink\n"
  continue
end
break *0x10148f04 if $confirm == 1
commands
  silent
  set $confirm = 2
  printf "CONFIRMED reset\n"
  delete 3 4
  monitor system_reset
  continue
end
break *0x100d6e42
commands
  silent
  printf "INIT id=%u\n", *(unsigned *)0x38026cb4
  continue
end
continue
