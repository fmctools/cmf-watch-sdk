# Watch 3 Pro guard e2e (tests/test_wfguard_e2e.py): models bad built-in faces with breakpoints on the stock
# jx402_08 1.0.1.77 and walks every hook. Run after `target remote` with $udf (a `udf` in the stub) and $loaded,
# $survived (the stub's timer callbacks).
set $faulted = 0
set $timer = 0
set $loads = 0
set $erase = 0
set $picked = 0
# the first default list: entry 1 (276) active, as if picked (0x100D7BA2: bl WfmPara_Save, r4 = para)
break *0x100d7ba2 if $faulted == 0
commands
  silent
  set *(unsigned char *)($r4 + 5) = 1
  set *(unsigned char *)($r4 + 6) = *(unsigned char *)($r4 + 0xC + 16 + 4)
  set *(unsigned *)($r4 + 8) = *(unsigned *)($r4 + 0xC + 16)
  continue
end
# 276 is bad: a fault once WFManager_Init, past the guard's hook, has picked it
break *0x100d6e42
commands
  silent
  printf "INIT id=%u\n", *(unsigned *)0x38026cb4
  if *(unsigned *)0x38026cb4 == 276
    set $faulted = 1
    printf "BADFACE fault id=276\n"
    set $pc = $udf
  end
  continue
end
# the saved list is lost (as after a reset that skipped the flush): the default one is built, 274 active
break *0x100d6df4 if $erase == 1
commands
  silent
  set $erase = 0
  set *(unsigned *)0x3800f73a = 0
  printf "LIST lost\n"
  continue
end
# every flag the stub writes
break *0x1011617c if $r2 == 8 && *(unsigned char *)$r1 == 0x47
commands
  silent
  printf "FLAG id=%u stage=%u deaths=%u\n", *(unsigned *)($r1 + 4), *(unsigned char *)($r1 + 1), *(unsigned char *)($r1 + 2)
  continue
end
break *$loaded
commands
  silent
  set $timer = 1
  set $loads = $loads + 1
  continue
end
break *$survived
commands
  silent
  set $timer = 2
  continue
end
# lv_timer_del at the end of the stub's callback (ui_service): the next step of the scenario
break *0x10148f04 if $timer != 0
commands
  silent
  set $ran = $timer
  set $timer = 0
  if $ran == 2 && $picked == 0
    set $picked = 1
    printf "SURVIVED, pick 276\n"
    set *(unsigned char *)0x38026db0 = 1
    set *(unsigned *)0x38026dc0 = 276
    set $r0 = 0
    set $r1 = 0
    set $r2 = 0
    set $pc = 0x100d6e9c
  end
  if $ran == 1 && $loads == 2
    printf "REBOOT menu\n"
    # ux2sys reboot, as the menu: system_power_reboot waits for ui_service, so it must run on the launcher
    set $pc = 0x100da80c
  end
  if $ran == 1 && $loads == 3
    printf "REBOOT ota\n"
    set $r0 = 2
    set $pc = 0x10155308
  end
  if $ran == 1 && $loads >= 4 && $loads <= 9
    printf "RESET\n"
    if $loads >= 6
      set $erase = 1
    end
    monitor system_reset
  end
  continue
end
# the pick (WFM PendHandle's SetCurr, as the phone's 0x9055) saved the list and asked the face view to reload: 276
# faults as the parser (0x100D48B4, past the stub's hook) gets it
break *0x100d6f7c if $picked == 1
commands
  silent
  printf "PICKED saved id=%u\n", *(unsigned *)0x38026cb4
  continue
end
break *0x100d48b4 if $picked == 1 && *(unsigned *)0x38026cb4 == 276
commands
  silent
  set $picked = 2
  printf "PICKED fault id=%u\n", *(unsigned *)0x38026cb4
  set $pc = $udf
  continue
end
continue
