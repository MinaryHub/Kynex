MODULE KYNEX_UR10e_1
  ! Made by KYNEX from program "UR10e 1".
  ! Targets in the base frame (wobj0), millimetres and quaternions.
  ! Signals to set up on the controller:
  !   DO_KYNEX_0: digital output
  !   DO_KYNEX_1: digital output
  !   DO_KYNEX_2: digital output
  !   DO_KYNEX_3: digital output
  !   DO_KYNEX_4: digital output
  !   DO_KYNEX_5: digital output
  !   DO_KYNEX_6: digital output
  !   DO_KYNEX_8: digital output
  !   DO_KYNEX_9: digital output
  !   DO_KYNEX_10: digital output
  !   DO_KYNEX_11: digital output
  !   DO_KYNEX_12: digital output
  PERS tooldata kynex_tool := [TRUE,[[0.00,70.00,170.00],[1.000000,0.000000,0.000000,0.000000]],[1,[0,0,1],[1,0,0,0],0,0,0]];
  CONST speeddata kynex_v250 := [250,500,5000,1000];
  ! The places of pallet
  CONST pos kynex_places_0{20} := [[1000.00,100.00,110.00],[900.00,100.00,110.00],[800.00,100.00,110.00],[700.00,100.00,110.00],[600.00,100.00,110.00],[1000.00,0.00,110.00],[900.00,0.00,110.00],[800.00,0.00,110.00],[700.00,0.00,110.00],[600.00,0.00,110.00],[1000.00,-100.00,110.00],[900.00,-100.00,110.00],[800.00,-100.00,110.00],[700.00,-100.00,110.00],[600.00,-100.00,110.00],[1000.00,-200.00,110.00],[900.00,-200.00,110.00],[800.00,-200.00,110.00],[700.00,-200.00,110.00],[600.00,-200.00,110.00]];
  VAR num kynex_place_0 := 0;
  ! The places of pallet
  CONST pos kynex_places_1{20} := [[200.00,950.00,110.00],[100.00,950.00,110.00],[0.00,950.00,110.00],[-100.00,950.00,110.00],[-200.00,950.00,110.00],[200.00,850.00,110.00],[100.00,850.00,110.00],[0.00,850.00,110.00],[-100.00,850.00,110.00],[-200.00,850.00,110.00],[200.00,750.00,110.00],[100.00,750.00,110.00],[0.00,750.00,110.00],[-100.00,750.00,110.00],[-200.00,750.00,110.00],[200.00,650.00,110.00],[100.00,650.00,110.00],[0.00,650.00,110.00],[-100.00,650.00,110.00],[-200.00,650.00,110.00]];
  VAR num kynex_place_1 := 0;
  PERS num kynex_mark := 0;
  PERS num kynex_done := 0;
  ! P1
  CONST jointtarget j1 := [[171.6937,-58.9191,75.0599,-106.1409,-90.0000,81.6937],[9E9,9E9,9E9,9E9,9E9,9E9]];
  CONST robtarget p1 := [[1000.00,100.00,260.00],[0.000001,0.000000,1.000000,0.000000],[0,0,0,0],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P2
  CONST robtarget p2 := [[1000.00,100.00,110.00],[0.000003,0.000000,1.000000,0.000000],[0,0,0,0],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P3
  CONST robtarget p3 := [[1000.00,100.00,260.00],[0.000004,0.000001,1.000000,0.000000],[0,0,0,0],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P4
  CONST jointtarget j4 := [[61.9219,-126.0517,118.9673,-82.9156,-90.0000,-28.0781],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P5
  CONST robtarget p5 := [[0.00,-1100.00,460.00],[0.000000,0.000000,1.000000,-0.000009],[0,0,0,0],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P6
  CONST robtarget p6 := [[0.00,-1100.00,310.00],[0.000000,0.000000,1.000000,-0.000001],[0,0,0,0],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P7
  CONST robtarget p7 := [[0.00,-1099.99,460.00],[0.000000,0.000000,1.000000,-0.000013],[0,0,0,0],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P8
  CONST robtarget p8 := [[0.00,-300.00,460.00],[0.000001,0.000000,-1.000000,0.000000],[0,0,0,0],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P9
  CONST robtarget p9 := [[999.99,100.00,260.00],[0.000020,0.000001,1.000000,0.000001],[0,0,0,0],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P10
  CONST robtarget p10 := [[1000.00,100.00,110.00],[0.000003,0.000000,1.000000,0.000000],[0,0,0,0],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P11
  CONST robtarget p11 := [[1000.00,100.00,260.00],[0.000004,0.000001,1.000000,0.000000],[0,0,0,0],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P12
  CONST jointtarget j12 := [[338.7101,-133.7661,121.2999,-77.5339,-90.0001,248.7102],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P13
  CONST robtarget p13 := [[-1100.00,0.00,460.00],[0.000002,0.000000,-1.000000,0.000000],[0,0,0,0],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P14
  CONST robtarget p14 := [[-1100.00,0.00,310.00],[0.000009,0.000002,-1.000000,0.000000],[0,0,0,0],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P15
  CONST robtarget p15 := [[-1100.00,0.00,460.00],[0.000002,0.000000,-1.000000,0.000000],[0,0,0,0],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P16
  CONST robtarget p16 := [[-300.00,-0.01,460.00],[0.000004,0.000012,1.000000,0.000023],[0,0,0,0],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P17
  CONST robtarget p17 := [[1000.00,100.00,260.00],[0.000001,0.000000,1.000000,0.000000],[0,0,0,0],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P18
  CONST robtarget p18 := [[1000.00,100.00,110.00],[0.000003,0.000000,1.000000,0.000000],[0,0,0,0],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P19
  CONST robtarget p19 := [[1000.00,100.00,260.00],[0.000004,0.000001,1.000000,0.000000],[0,0,0,0],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P1
  CONST jointtarget j20 := [[40.7878,-132.2265,92.9082,-50.6817,-90.0009,-49.2118],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P2
  CONST robtarget p21 := [[0.00,-160.00,460.00],[0.000000,0.000000,1.000000,-0.000001],[0,0,0,0],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P3
  CONST robtarget p22 := [[0.00,-960.00,460.00],[0.000000,0.000000,1.000000,-0.000001],[0,0,0,0],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P4
  CONST robtarget p23 := [[0.00,-960.00,307.50],[0.000001,0.000000,-1.000000,0.000010],[0,0,0,0],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P5
  CONST robtarget p24 := [[0.00,-959.99,459.99],[0.000001,0.000000,-1.000000,0.000016],[0,0,0,0],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P6
  CONST robtarget p25 := [[0.00,-1099.99,460.00],[0.000000,0.000000,1.000000,-0.000010],[0,0,0,0],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P7
  CONST robtarget p26 := [[0.00,-1100.00,310.00],[0.000000,0.000000,1.000000,-0.000001],[0,0,0,0],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P8
  CONST robtarget p27 := [[0.00,-1099.99,460.00],[0.000000,0.000000,1.000000,-0.000013],[0,0,0,0],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P9
  CONST robtarget p28 := [[0.00,-300.00,460.00],[0.000001,0.000000,-1.000000,0.000000],[0,0,0,0],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P10
  CONST robtarget p29 := [[200.00,1090.00,260.00],[0.000000,0.000000,1.000000,0.000000],[0,0,0,0],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P11
  CONST robtarget p30 := [[200.00,1090.00,110.00],[0.000001,0.000000,1.000000,0.000004],[0,0,0,0],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P12
  CONST robtarget p31 := [[200.00,1090.00,260.00],[0.000001,0.000000,1.000000,0.000005],[0,0,0,0],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P13
  CONST robtarget p32 := [[1000.00,100.00,260.00],[0.000002,0.000000,1.000000,0.000000],[0,0,0,0],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P14
  CONST robtarget p33 := [[1000.00,100.00,110.00],[0.000003,0.000000,1.000000,0.000000],[0,0,0,0],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P15
  CONST robtarget p34 := [[1000.00,100.00,260.00],[0.000004,0.000001,1.000000,0.000000],[0,0,0,0],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P16
  CONST jointtarget j35 := [[287.8127,-96.5594,81.6391,-75.0797,-90.0000,197.8127],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P17
  CONST jointtarget j36 := [[312.4435,-124.3660,91.5088,-57.1432,-90.0015,222.4441],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P18
  CONST robtarget p37 := [[-300.00,140.00,460.00],[0.000000,0.000000,1.000000,0.000000],[0,0,0,0],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P19
  CONST robtarget p38 := [[-1100.00,140.00,460.00],[0.000004,-0.000001,-1.000000,0.000000],[0,0,0,0],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P20
  CONST robtarget p39 := [[-1099.99,140.00,307.50],[0.000010,-0.000002,-1.000000,-0.000001],[0,0,0,0],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P21
  CONST robtarget p40 := [[-1100.00,140.00,460.00],[0.000002,0.000000,-1.000000,0.000000],[0,0,0,0],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P22
  CONST robtarget p41 := [[-1099.99,0.00,460.00],[0.000017,0.000003,-1.000000,-0.000001],[0,0,0,0],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P23
  CONST robtarget p42 := [[-1100.00,0.00,310.00],[0.000009,0.000002,-1.000000,0.000000],[0,0,0,0],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P24
  CONST robtarget p43 := [[-1100.00,0.00,460.00],[0.000002,0.000000,-1.000000,0.000000],[0,0,0,0],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P25
  CONST robtarget p44 := [[-300.00,-0.01,460.00],[0.000004,0.000012,1.000000,0.000023],[0,0,0,0],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P26
  CONST robtarget p45 := [[200.00,1090.00,260.00],[0.000003,0.000001,1.000000,0.000008],[0,0,0,0],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P27
  CONST robtarget p46 := [[200.00,1090.00,110.00],[0.000001,0.000000,1.000000,0.000004],[0,0,0,0],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P28
  CONST robtarget p47 := [[200.00,1090.00,260.00],[0.000001,0.000000,1.000000,0.000005],[0,0,0,0],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P29
  CONST robtarget p48 := [[1000.00,100.00,260.00],[0.000002,0.000000,1.000000,0.000000],[0,0,0,0],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P30
  CONST robtarget p49 := [[1000.00,100.00,110.00],[0.000003,0.000000,1.000000,0.000000],[0,0,0,0],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P31
  CONST robtarget p50 := [[1000.00,100.00,260.00],[0.000004,0.000001,1.000000,0.000000],[0,0,0,0],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P1
  CONST jointtarget j51 := [[40.7878,-132.2265,92.9082,-50.6817,-90.0009,-49.2118],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P2
  CONST robtarget p52 := [[0.00,-160.00,460.00],[0.000000,0.000000,1.000000,-0.000001],[0,0,0,0],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P3
  CONST robtarget p53 := [[0.00,-960.00,460.00],[0.000000,0.000000,1.000000,-0.000001],[0,0,0,0],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P4
  CONST robtarget p54 := [[0.00,-960.00,307.50],[0.000001,0.000000,-1.000000,0.000010],[0,0,0,0],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P5
  CONST robtarget p55 := [[0.00,-959.99,459.99],[0.000001,0.000000,-1.000000,0.000016],[0,0,0,0],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P6
  CONST robtarget p56 := [[0.00,-1099.99,460.00],[0.000000,0.000000,1.000000,-0.000010],[0,0,0,0],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P7
  CONST robtarget p57 := [[0.00,-1100.00,310.00],[0.000000,0.000000,1.000000,-0.000001],[0,0,0,0],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P8
  CONST robtarget p58 := [[0.00,-1099.99,460.00],[0.000000,0.000000,1.000000,-0.000013],[0,0,0,0],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P9
  CONST robtarget p59 := [[0.00,-300.00,460.00],[0.000001,0.000000,-1.000000,0.000000],[0,0,0,0],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P10
  CONST robtarget p60 := [[200.00,1090.00,260.00],[0.000000,0.000000,1.000000,0.000000],[0,0,0,0],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P11
  CONST robtarget p61 := [[200.00,1090.00,110.00],[0.000001,0.000000,1.000000,0.000004],[0,0,0,0],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P12
  CONST robtarget p62 := [[200.00,1090.00,260.00],[0.000001,0.000000,1.000000,0.000005],[0,0,0,0],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P13
  CONST robtarget p63 := [[1000.00,100.00,260.00],[0.000002,0.000000,1.000000,0.000000],[0,0,0,0],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P14
  CONST robtarget p64 := [[1000.00,100.00,110.00],[0.000003,0.000000,1.000000,0.000000],[0,0,0,0],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P15
  CONST robtarget p65 := [[1000.00,100.00,260.00],[0.000004,0.000001,1.000000,0.000000],[0,0,0,0],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P16
  CONST jointtarget j66 := [[287.8127,-96.5594,81.6391,-75.0797,-90.0000,197.8127],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P17
  CONST jointtarget j67 := [[312.4435,-124.3660,91.5088,-57.1432,-90.0015,222.4441],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P18
  CONST robtarget p68 := [[-300.00,140.00,460.00],[0.000000,0.000000,1.000000,0.000000],[0,0,0,0],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P19
  CONST robtarget p69 := [[-1100.00,140.00,460.00],[0.000004,-0.000001,-1.000000,0.000000],[0,0,0,0],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P20
  CONST robtarget p70 := [[-1099.99,140.00,307.50],[0.000010,-0.000002,-1.000000,-0.000001],[0,0,0,0],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P21
  CONST robtarget p71 := [[-1100.00,140.00,460.00],[0.000002,0.000000,-1.000000,0.000000],[0,0,0,0],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P22
  CONST robtarget p72 := [[-1099.99,0.00,460.00],[0.000017,0.000003,-1.000000,-0.000001],[0,0,0,0],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P23
  CONST robtarget p73 := [[-1100.00,0.00,310.00],[0.000009,0.000002,-1.000000,0.000000],[0,0,0,0],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P24
  CONST robtarget p74 := [[-1100.00,0.00,460.00],[0.000002,0.000000,-1.000000,0.000000],[0,0,0,0],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P25
  CONST robtarget p75 := [[-300.00,-0.01,460.00],[0.000004,0.000012,1.000000,0.000023],[0,0,0,0],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P26
  CONST robtarget p76 := [[200.00,1090.00,260.00],[0.000003,0.000001,1.000000,0.000008],[0,0,0,0],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P27
  CONST robtarget p77 := [[200.00,1090.00,110.00],[0.000001,0.000000,1.000000,0.000004],[0,0,0,0],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P28
  CONST robtarget p78 := [[200.00,1090.00,260.00],[0.000001,0.000000,1.000000,0.000005],[0,0,0,0],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P29
  CONST robtarget p79 := [[1000.00,100.00,260.00],[0.000002,0.000000,1.000000,0.000000],[0,0,0,0],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P30
  CONST robtarget p80 := [[1000.00,100.00,110.00],[0.000003,0.000000,1.000000,0.000000],[0,0,0,0],[9E9,9E9,9E9,9E9,9E9,9E9]];
  ! P31
  CONST robtarget p81 := [[1000.00,100.00,260.00],[0.000004,0.000001,1.000000,0.000000],[0,0,0,0],[9E9,9E9,9E9,9E9,9E9,9E9]];

  PROC main()
    ConfJ \Off;
    ConfL \Off;
    ! To the start
    kynex_mark := 1;
    ! P1
    VelSet 50, 5000;
    MoveAbsJ j1, vmax, fine, kynex_tool;
    kynex_done := 1;
    kynex_mark := 2;
    ! P2
    VelSet 100, 5000;
    MoveL Offs(p2, kynex_places_0{kynex_place_0 + 1}.x - kynex_places_0{1}.x, kynex_places_0{kynex_place_0 + 1}.y - kynex_places_0{1}.y, kynex_places_0{kynex_place_0 + 1}.z - kynex_places_0{1}.z), kynex_v250, fine, kynex_tool \WObj:=wobj0;
    kynex_done := 2;
    SetDO DO_KYNEX_0, 1;
    WaitTime 0.50;
    kynex_mark := 3;
    ! P3
    MoveL Offs(p3, kynex_places_0{kynex_place_0 + 1}.x - kynex_places_0{1}.x, kynex_places_0{kynex_place_0 + 1}.y - kynex_places_0{1}.y, kynex_places_0{kynex_place_0 + 1}.z - kynex_places_0{1}.z), kynex_v250, fine, kynex_tool \WObj:=wobj0;
    kynex_done := 3;
    kynex_place_0 := (kynex_place_0 + 1) MOD 20;
    PulseDO \PLength:=0.2, DO_KYNEX_2;
    ! KYNEX, not exported until its I/O is mapped: wait for CNC ?? 1 to say door_open
    kynex_mark := 4;
    ! P4
    VelSet 50, 5000;
    MoveAbsJ j4, vmax, fine, kynex_tool;
    kynex_done := 4;
    kynex_mark := 5;
    ! P5
    VelSet 100, 5000;
    MoveL p5, kynex_v250, fine, kynex_tool \WObj:=wobj0;
    kynex_done := 5;
    kynex_mark := 6;
    ! P6
    MoveL p6, kynex_v250, fine, kynex_tool \WObj:=wobj0;
    kynex_done := 6;
    SetDO DO_KYNEX_0, 0;
    WaitTime 0.50;
    kynex_mark := 7;
    ! P7
    MoveL p7, kynex_v250, fine, kynex_tool \WObj:=wobj0;
    kynex_done := 7;
    kynex_mark := 8;
    ! P8
    MoveL p8, kynex_v250, fine, kynex_tool \WObj:=wobj0;
    kynex_done := 8;
    PulseDO \PLength:=0.2, DO_KYNEX_4;
    PulseDO \PLength:=0.2, DO_KYNEX_3;
    ! KYNEX, not exported until its I/O is mapped: wait for CNC ?? 1 to say door_closed
    PulseDO \PLength:=0.2, DO_KYNEX_6;
    kynex_mark := 9;
    ! P9
    VelSet 50, 5000;
    MoveJ Offs(p9, kynex_places_0{kynex_place_0 + 1}.x - kynex_places_0{1}.x, kynex_places_0{kynex_place_0 + 1}.y - kynex_places_0{1}.y, kynex_places_0{kynex_place_0 + 1}.z - kynex_places_0{1}.z), vmax, fine, kynex_tool \WObj:=wobj0;
    kynex_done := 9;
    kynex_mark := 10;
    ! P10
    VelSet 100, 5000;
    MoveL Offs(p10, kynex_places_0{kynex_place_0 + 1}.x - kynex_places_0{1}.x, kynex_places_0{kynex_place_0 + 1}.y - kynex_places_0{1}.y, kynex_places_0{kynex_place_0 + 1}.z - kynex_places_0{1}.z), kynex_v250, fine, kynex_tool \WObj:=wobj0;
    kynex_done := 10;
    SetDO DO_KYNEX_0, 1;
    WaitTime 0.50;
    kynex_mark := 11;
    ! P11
    MoveL Offs(p11, kynex_places_0{kynex_place_0 + 1}.x - kynex_places_0{1}.x, kynex_places_0{kynex_place_0 + 1}.y - kynex_places_0{1}.y, kynex_places_0{kynex_place_0 + 1}.z - kynex_places_0{1}.z), kynex_v250, fine, kynex_tool \WObj:=wobj0;
    kynex_done := 11;
    kynex_place_0 := (kynex_place_0 + 1) MOD 20;
    PulseDO \PLength:=0.2, DO_KYNEX_8;
    ! KYNEX, not exported until its I/O is mapped: wait for CNC ?? 2 to say door_open
    kynex_mark := 12;
    ! P12
    VelSet 50, 5000;
    MoveAbsJ j12, vmax, fine, kynex_tool;
    kynex_done := 12;
    kynex_mark := 13;
    ! P13
    VelSet 100, 5000;
    MoveL p13, kynex_v250, fine, kynex_tool \WObj:=wobj0;
    kynex_done := 13;
    kynex_mark := 14;
    ! P14
    MoveL p14, kynex_v250, fine, kynex_tool \WObj:=wobj0;
    kynex_done := 14;
    SetDO DO_KYNEX_0, 0;
    WaitTime 0.50;
    kynex_mark := 15;
    ! P15
    MoveL p15, kynex_v250, fine, kynex_tool \WObj:=wobj0;
    kynex_done := 15;
    kynex_mark := 16;
    ! P16
    MoveL p16, kynex_v250, fine, kynex_tool \WObj:=wobj0;
    kynex_done := 16;
    PulseDO \PLength:=0.2, DO_KYNEX_10;
    PulseDO \PLength:=0.2, DO_KYNEX_9;
    ! KYNEX, not exported until its I/O is mapped: wait for CNC ?? 2 to say door_closed
    PulseDO \PLength:=0.2, DO_KYNEX_12;
    kynex_mark := 17;
    ! P17
    VelSet 50, 5000;
    MoveJ Offs(p17, kynex_places_0{kynex_place_0 + 1}.x - kynex_places_0{1}.x, kynex_places_0{kynex_place_0 + 1}.y - kynex_places_0{1}.y, kynex_places_0{kynex_place_0 + 1}.z - kynex_places_0{1}.z), vmax, fine, kynex_tool \WObj:=wobj0;
    kynex_done := 17;
    kynex_mark := 18;
    ! P18
    VelSet 100, 5000;
    MoveL Offs(p18, kynex_places_0{kynex_place_0 + 1}.x - kynex_places_0{1}.x, kynex_places_0{kynex_place_0 + 1}.y - kynex_places_0{1}.y, kynex_places_0{kynex_place_0 + 1}.z - kynex_places_0{1}.z), kynex_v250, fine, kynex_tool \WObj:=wobj0;
    kynex_done := 18;
    SetDO DO_KYNEX_0, 1;
    WaitTime 0.50;
    kynex_mark := 19;
    ! P19
    MoveL Offs(p19, kynex_places_0{kynex_place_0 + 1}.x - kynex_places_0{1}.x, kynex_places_0{kynex_place_0 + 1}.y - kynex_places_0{1}.y, kynex_places_0{kynex_place_0 + 1}.z - kynex_places_0{1}.z), kynex_v250, fine, kynex_tool \WObj:=wobj0;
    kynex_done := 19;
    kynex_place_0 := (kynex_place_0 + 1) MOD 20;
    ! KYNEX, not exported until its I/O is mapped: wait for CNC ?? 1 to say done
    PulseDO \PLength:=0.2, DO_KYNEX_2;
    ! KYNEX, not exported until its I/O is mapped: wait for CNC ?? 1 to say door_open
    PulseDO \PLength:=0.2, DO_KYNEX_5;
    kynex_mark := 20;
    ! P1
    VelSet 50, 5000;
    MoveAbsJ j20, vmax, fine, kynex_tool;
    kynex_done := 20;
    kynex_mark := 21;
    ! P2
    VelSet 100, 5000;
    MoveL p21, kynex_v250, fine, kynex_tool \WObj:=wobj0;
    kynex_done := 21;
    kynex_mark := 22;
    ! P3
    MoveL p22, kynex_v250, fine, kynex_tool \WObj:=wobj0;
    kynex_done := 22;
    kynex_mark := 23;
    ! P4
    MoveL p23, kynex_v250, fine, kynex_tool \WObj:=wobj0;
    kynex_done := 23;
    SetDO DO_KYNEX_1, 1;
    WaitTime 0.50;
    kynex_mark := 24;
    ! P5
    MoveL p24, kynex_v250, fine, kynex_tool \WObj:=wobj0;
    kynex_done := 24;
    kynex_mark := 25;
    ! P6
    MoveL p25, kynex_v250, fine, kynex_tool \WObj:=wobj0;
    kynex_done := 25;
    kynex_mark := 26;
    ! P7
    MoveL p26, kynex_v250, fine, kynex_tool \WObj:=wobj0;
    kynex_done := 26;
    SetDO DO_KYNEX_0, 0;
    WaitTime 0.50;
    kynex_mark := 27;
    ! P8
    MoveL p27, kynex_v250, fine, kynex_tool \WObj:=wobj0;
    kynex_done := 27;
    kynex_mark := 28;
    ! P9
    MoveL p28, kynex_v250, fine, kynex_tool \WObj:=wobj0;
    kynex_done := 28;
    PulseDO \PLength:=0.2, DO_KYNEX_4;
    PulseDO \PLength:=0.2, DO_KYNEX_3;
    ! KYNEX, not exported until its I/O is mapped: wait for CNC ?? 1 to say door_closed
    PulseDO \PLength:=0.2, DO_KYNEX_6;
    kynex_mark := 29;
    ! P10
    VelSet 50, 5000;
    MoveJ Offs(p29, kynex_places_1{kynex_place_1 + 1}.x - kynex_places_1{1}.x, kynex_places_1{kynex_place_1 + 1}.y - kynex_places_1{1}.y, kynex_places_1{kynex_place_1 + 1}.z - kynex_places_1{1}.z), vmax, fine, kynex_tool \WObj:=wobj0;
    kynex_done := 29;
    kynex_mark := 30;
    ! P11
    VelSet 100, 5000;
    MoveL Offs(p30, kynex_places_1{kynex_place_1 + 1}.x - kynex_places_1{1}.x, kynex_places_1{kynex_place_1 + 1}.y - kynex_places_1{1}.y, kynex_places_1{kynex_place_1 + 1}.z - kynex_places_1{1}.z), kynex_v250, fine, kynex_tool \WObj:=wobj0;
    kynex_done := 30;
    SetDO DO_KYNEX_1, 0;
    WaitTime 0.50;
    kynex_mark := 31;
    ! P12
    MoveL Offs(p31, kynex_places_1{kynex_place_1 + 1}.x - kynex_places_1{1}.x, kynex_places_1{kynex_place_1 + 1}.y - kynex_places_1{1}.y, kynex_places_1{kynex_place_1 + 1}.z - kynex_places_1{1}.z), kynex_v250, fine, kynex_tool \WObj:=wobj0;
    kynex_done := 31;
    kynex_place_1 := (kynex_place_1 + 1) MOD 20;
    kynex_mark := 32;
    ! P13
    VelSet 50, 5000;
    MoveJ Offs(p32, kynex_places_0{kynex_place_0 + 1}.x - kynex_places_0{1}.x, kynex_places_0{kynex_place_0 + 1}.y - kynex_places_0{1}.y, kynex_places_0{kynex_place_0 + 1}.z - kynex_places_0{1}.z), vmax, fine, kynex_tool \WObj:=wobj0;
    kynex_done := 32;
    kynex_mark := 33;
    ! P14
    VelSet 100, 5000;
    MoveL Offs(p33, kynex_places_0{kynex_place_0 + 1}.x - kynex_places_0{1}.x, kynex_places_0{kynex_place_0 + 1}.y - kynex_places_0{1}.y, kynex_places_0{kynex_place_0 + 1}.z - kynex_places_0{1}.z), kynex_v250, fine, kynex_tool \WObj:=wobj0;
    kynex_done := 33;
    SetDO DO_KYNEX_0, 1;
    WaitTime 0.50;
    kynex_mark := 34;
    ! P15
    MoveL Offs(p34, kynex_places_0{kynex_place_0 + 1}.x - kynex_places_0{1}.x, kynex_places_0{kynex_place_0 + 1}.y - kynex_places_0{1}.y, kynex_places_0{kynex_place_0 + 1}.z - kynex_places_0{1}.z), kynex_v250, fine, kynex_tool \WObj:=wobj0;
    kynex_done := 34;
    kynex_place_0 := (kynex_place_0 + 1) MOD 20;
    ! KYNEX, not exported until its I/O is mapped: wait for CNC ?? 2 to say done
    PulseDO \PLength:=0.2, DO_KYNEX_8;
    ! KYNEX, not exported until its I/O is mapped: wait for CNC ?? 2 to say door_open
    PulseDO \PLength:=0.2, DO_KYNEX_11;
    kynex_mark := 35;
    ! P16
    VelSet 50, 5000;
    MoveAbsJ j35, vmax, fine, kynex_tool;
    kynex_done := 35;
    kynex_mark := 36;
    ! P17
    MoveAbsJ j36, vmax, fine, kynex_tool;
    kynex_done := 36;
    kynex_mark := 37;
    ! P18
    VelSet 100, 5000;
    MoveL p37, kynex_v250, fine, kynex_tool \WObj:=wobj0;
    kynex_done := 37;
    kynex_mark := 38;
    ! P19
    MoveL p38, kynex_v250, fine, kynex_tool \WObj:=wobj0;
    kynex_done := 38;
    kynex_mark := 39;
    ! P20
    MoveL p39, kynex_v250, fine, kynex_tool \WObj:=wobj0;
    kynex_done := 39;
    SetDO DO_KYNEX_1, 1;
    WaitTime 0.50;
    kynex_mark := 40;
    ! P21
    MoveL p40, kynex_v250, fine, kynex_tool \WObj:=wobj0;
    kynex_done := 40;
    kynex_mark := 41;
    ! P22
    MoveL p41, kynex_v250, fine, kynex_tool \WObj:=wobj0;
    kynex_done := 41;
    kynex_mark := 42;
    ! P23
    MoveL p42, kynex_v250, fine, kynex_tool \WObj:=wobj0;
    kynex_done := 42;
    SetDO DO_KYNEX_0, 0;
    WaitTime 0.50;
    kynex_mark := 43;
    ! P24
    MoveL p43, kynex_v250, fine, kynex_tool \WObj:=wobj0;
    kynex_done := 43;
    kynex_mark := 44;
    ! P25
    MoveL p44, kynex_v250, fine, kynex_tool \WObj:=wobj0;
    kynex_done := 44;
    PulseDO \PLength:=0.2, DO_KYNEX_10;
    PulseDO \PLength:=0.2, DO_KYNEX_9;
    ! KYNEX, not exported until its I/O is mapped: wait for CNC ?? 2 to say door_closed
    PulseDO \PLength:=0.2, DO_KYNEX_12;
    kynex_mark := 45;
    ! P26
    VelSet 50, 5000;
    MoveJ Offs(p45, kynex_places_1{kynex_place_1 + 1}.x - kynex_places_1{1}.x, kynex_places_1{kynex_place_1 + 1}.y - kynex_places_1{1}.y, kynex_places_1{kynex_place_1 + 1}.z - kynex_places_1{1}.z), vmax, fine, kynex_tool \WObj:=wobj0;
    kynex_done := 45;
    kynex_mark := 46;
    ! P27
    VelSet 100, 5000;
    MoveL Offs(p46, kynex_places_1{kynex_place_1 + 1}.x - kynex_places_1{1}.x, kynex_places_1{kynex_place_1 + 1}.y - kynex_places_1{1}.y, kynex_places_1{kynex_place_1 + 1}.z - kynex_places_1{1}.z), kynex_v250, fine, kynex_tool \WObj:=wobj0;
    kynex_done := 46;
    SetDO DO_KYNEX_1, 0;
    WaitTime 0.50;
    kynex_mark := 47;
    ! P28
    MoveL Offs(p47, kynex_places_1{kynex_place_1 + 1}.x - kynex_places_1{1}.x, kynex_places_1{kynex_place_1 + 1}.y - kynex_places_1{1}.y, kynex_places_1{kynex_place_1 + 1}.z - kynex_places_1{1}.z), kynex_v250, fine, kynex_tool \WObj:=wobj0;
    kynex_done := 47;
    kynex_place_1 := (kynex_place_1 + 1) MOD 20;
    kynex_mark := 48;
    ! P29
    VelSet 50, 5000;
    MoveJ Offs(p48, kynex_places_0{kynex_place_0 + 1}.x - kynex_places_0{1}.x, kynex_places_0{kynex_place_0 + 1}.y - kynex_places_0{1}.y, kynex_places_0{kynex_place_0 + 1}.z - kynex_places_0{1}.z), vmax, fine, kynex_tool \WObj:=wobj0;
    kynex_done := 48;
    kynex_mark := 49;
    ! P30
    VelSet 100, 5000;
    MoveL Offs(p49, kynex_places_0{kynex_place_0 + 1}.x - kynex_places_0{1}.x, kynex_places_0{kynex_place_0 + 1}.y - kynex_places_0{1}.y, kynex_places_0{kynex_place_0 + 1}.z - kynex_places_0{1}.z), kynex_v250, fine, kynex_tool \WObj:=wobj0;
    kynex_done := 49;
    SetDO DO_KYNEX_0, 1;
    WaitTime 0.50;
    kynex_mark := 50;
    ! P31
    MoveL Offs(p50, kynex_places_0{kynex_place_0 + 1}.x - kynex_places_0{1}.x, kynex_places_0{kynex_place_0 + 1}.y - kynex_places_0{1}.y, kynex_places_0{kynex_place_0 + 1}.z - kynex_places_0{1}.z), kynex_v250, fine, kynex_tool \WObj:=wobj0;
    kynex_done := 50;
    kynex_place_0 := (kynex_place_0 + 1) MOD 20;
    ! KYNEX, not exported until its I/O is mapped: wait for CNC ?? 1 to say done
    PulseDO \PLength:=0.2, DO_KYNEX_2;
    ! KYNEX, not exported until its I/O is mapped: wait for CNC ?? 1 to say door_open
    PulseDO \PLength:=0.2, DO_KYNEX_5;
    kynex_mark := 51;
    ! P1
    VelSet 50, 5000;
    MoveAbsJ j51, vmax, fine, kynex_tool;
    kynex_done := 51;
    kynex_mark := 52;
    ! P2
    VelSet 100, 5000;
    MoveL p52, kynex_v250, fine, kynex_tool \WObj:=wobj0;
    kynex_done := 52;
    kynex_mark := 53;
    ! P3
    MoveL p53, kynex_v250, fine, kynex_tool \WObj:=wobj0;
    kynex_done := 53;
    kynex_mark := 54;
    ! P4
    MoveL p54, kynex_v250, fine, kynex_tool \WObj:=wobj0;
    kynex_done := 54;
    SetDO DO_KYNEX_1, 1;
    WaitTime 0.50;
    kynex_mark := 55;
    ! P5
    MoveL p55, kynex_v250, fine, kynex_tool \WObj:=wobj0;
    kynex_done := 55;
    kynex_mark := 56;
    ! P6
    MoveL p56, kynex_v250, fine, kynex_tool \WObj:=wobj0;
    kynex_done := 56;
    kynex_mark := 57;
    ! P7
    MoveL p57, kynex_v250, fine, kynex_tool \WObj:=wobj0;
    kynex_done := 57;
    SetDO DO_KYNEX_0, 0;
    WaitTime 0.50;
    kynex_mark := 58;
    ! P8
    MoveL p58, kynex_v250, fine, kynex_tool \WObj:=wobj0;
    kynex_done := 58;
    kynex_mark := 59;
    ! P9
    MoveL p59, kynex_v250, fine, kynex_tool \WObj:=wobj0;
    kynex_done := 59;
    PulseDO \PLength:=0.2, DO_KYNEX_4;
    PulseDO \PLength:=0.2, DO_KYNEX_3;
    ! KYNEX, not exported until its I/O is mapped: wait for CNC ?? 1 to say door_closed
    PulseDO \PLength:=0.2, DO_KYNEX_6;
    kynex_mark := 60;
    ! P10
    VelSet 50, 5000;
    MoveJ Offs(p60, kynex_places_1{kynex_place_1 + 1}.x - kynex_places_1{1}.x, kynex_places_1{kynex_place_1 + 1}.y - kynex_places_1{1}.y, kynex_places_1{kynex_place_1 + 1}.z - kynex_places_1{1}.z), vmax, fine, kynex_tool \WObj:=wobj0;
    kynex_done := 60;
    kynex_mark := 61;
    ! P11
    VelSet 100, 5000;
    MoveL Offs(p61, kynex_places_1{kynex_place_1 + 1}.x - kynex_places_1{1}.x, kynex_places_1{kynex_place_1 + 1}.y - kynex_places_1{1}.y, kynex_places_1{kynex_place_1 + 1}.z - kynex_places_1{1}.z), kynex_v250, fine, kynex_tool \WObj:=wobj0;
    kynex_done := 61;
    SetDO DO_KYNEX_1, 0;
    WaitTime 0.50;
    kynex_mark := 62;
    ! P12
    MoveL Offs(p62, kynex_places_1{kynex_place_1 + 1}.x - kynex_places_1{1}.x, kynex_places_1{kynex_place_1 + 1}.y - kynex_places_1{1}.y, kynex_places_1{kynex_place_1 + 1}.z - kynex_places_1{1}.z), kynex_v250, fine, kynex_tool \WObj:=wobj0;
    kynex_done := 62;
    kynex_place_1 := (kynex_place_1 + 1) MOD 20;
    kynex_mark := 63;
    ! P13
    VelSet 50, 5000;
    MoveJ Offs(p63, kynex_places_0{kynex_place_0 + 1}.x - kynex_places_0{1}.x, kynex_places_0{kynex_place_0 + 1}.y - kynex_places_0{1}.y, kynex_places_0{kynex_place_0 + 1}.z - kynex_places_0{1}.z), vmax, fine, kynex_tool \WObj:=wobj0;
    kynex_done := 63;
    kynex_mark := 64;
    ! P14
    VelSet 100, 5000;
    MoveL Offs(p64, kynex_places_0{kynex_place_0 + 1}.x - kynex_places_0{1}.x, kynex_places_0{kynex_place_0 + 1}.y - kynex_places_0{1}.y, kynex_places_0{kynex_place_0 + 1}.z - kynex_places_0{1}.z), kynex_v250, fine, kynex_tool \WObj:=wobj0;
    kynex_done := 64;
    SetDO DO_KYNEX_0, 1;
    WaitTime 0.50;
    kynex_mark := 65;
    ! P15
    MoveL Offs(p65, kynex_places_0{kynex_place_0 + 1}.x - kynex_places_0{1}.x, kynex_places_0{kynex_place_0 + 1}.y - kynex_places_0{1}.y, kynex_places_0{kynex_place_0 + 1}.z - kynex_places_0{1}.z), kynex_v250, fine, kynex_tool \WObj:=wobj0;
    kynex_done := 65;
    kynex_place_0 := (kynex_place_0 + 1) MOD 20;
    ! KYNEX, not exported until its I/O is mapped: wait for CNC ?? 2 to say done
    PulseDO \PLength:=0.2, DO_KYNEX_8;
    ! KYNEX, not exported until its I/O is mapped: wait for CNC ?? 2 to say door_open
    PulseDO \PLength:=0.2, DO_KYNEX_11;
    kynex_mark := 66;
    ! P16
    VelSet 50, 5000;
    MoveAbsJ j66, vmax, fine, kynex_tool;
    kynex_done := 66;
    kynex_mark := 67;
    ! P17
    MoveAbsJ j67, vmax, fine, kynex_tool;
    kynex_done := 67;
    kynex_mark := 68;
    ! P18
    VelSet 100, 5000;
    MoveL p68, kynex_v250, fine, kynex_tool \WObj:=wobj0;
    kynex_done := 68;
    kynex_mark := 69;
    ! P19
    MoveL p69, kynex_v250, fine, kynex_tool \WObj:=wobj0;
    kynex_done := 69;
    kynex_mark := 70;
    ! P20
    MoveL p70, kynex_v250, fine, kynex_tool \WObj:=wobj0;
    kynex_done := 70;
    SetDO DO_KYNEX_1, 1;
    WaitTime 0.50;
    kynex_mark := 71;
    ! P21
    MoveL p71, kynex_v250, fine, kynex_tool \WObj:=wobj0;
    kynex_done := 71;
    kynex_mark := 72;
    ! P22
    MoveL p72, kynex_v250, fine, kynex_tool \WObj:=wobj0;
    kynex_done := 72;
    kynex_mark := 73;
    ! P23
    MoveL p73, kynex_v250, fine, kynex_tool \WObj:=wobj0;
    kynex_done := 73;
    SetDO DO_KYNEX_0, 0;
    WaitTime 0.50;
    kynex_mark := 74;
    ! P24
    MoveL p74, kynex_v250, fine, kynex_tool \WObj:=wobj0;
    kynex_done := 74;
    kynex_mark := 75;
    ! P25
    MoveL p75, kynex_v250, fine, kynex_tool \WObj:=wobj0;
    kynex_done := 75;
    PulseDO \PLength:=0.2, DO_KYNEX_10;
    PulseDO \PLength:=0.2, DO_KYNEX_9;
    ! KYNEX, not exported until its I/O is mapped: wait for CNC ?? 2 to say door_closed
    PulseDO \PLength:=0.2, DO_KYNEX_12;
    kynex_mark := 76;
    ! P26
    VelSet 50, 5000;
    MoveJ Offs(p76, kynex_places_1{kynex_place_1 + 1}.x - kynex_places_1{1}.x, kynex_places_1{kynex_place_1 + 1}.y - kynex_places_1{1}.y, kynex_places_1{kynex_place_1 + 1}.z - kynex_places_1{1}.z), vmax, fine, kynex_tool \WObj:=wobj0;
    kynex_done := 76;
    kynex_mark := 77;
    ! P27
    VelSet 100, 5000;
    MoveL Offs(p77, kynex_places_1{kynex_place_1 + 1}.x - kynex_places_1{1}.x, kynex_places_1{kynex_place_1 + 1}.y - kynex_places_1{1}.y, kynex_places_1{kynex_place_1 + 1}.z - kynex_places_1{1}.z), kynex_v250, fine, kynex_tool \WObj:=wobj0;
    kynex_done := 77;
    SetDO DO_KYNEX_1, 0;
    WaitTime 0.50;
    kynex_mark := 78;
    ! P28
    MoveL Offs(p78, kynex_places_1{kynex_place_1 + 1}.x - kynex_places_1{1}.x, kynex_places_1{kynex_place_1 + 1}.y - kynex_places_1{1}.y, kynex_places_1{kynex_place_1 + 1}.z - kynex_places_1{1}.z), kynex_v250, fine, kynex_tool \WObj:=wobj0;
    kynex_done := 78;
    kynex_place_1 := (kynex_place_1 + 1) MOD 20;
    kynex_mark := 79;
    ! P29
    VelSet 50, 5000;
    MoveJ Offs(p79, kynex_places_0{kynex_place_0 + 1}.x - kynex_places_0{1}.x, kynex_places_0{kynex_place_0 + 1}.y - kynex_places_0{1}.y, kynex_places_0{kynex_place_0 + 1}.z - kynex_places_0{1}.z), vmax, fine, kynex_tool \WObj:=wobj0;
    kynex_done := 79;
    kynex_mark := 80;
    ! P30
    VelSet 100, 5000;
    MoveL Offs(p80, kynex_places_0{kynex_place_0 + 1}.x - kynex_places_0{1}.x, kynex_places_0{kynex_place_0 + 1}.y - kynex_places_0{1}.y, kynex_places_0{kynex_place_0 + 1}.z - kynex_places_0{1}.z), kynex_v250, fine, kynex_tool \WObj:=wobj0;
    kynex_done := 80;
    SetDO DO_KYNEX_0, 1;
    WaitTime 0.50;
    kynex_mark := 81;
    ! P31
    MoveL Offs(p81, kynex_places_0{kynex_place_0 + 1}.x - kynex_places_0{1}.x, kynex_places_0{kynex_place_0 + 1}.y - kynex_places_0{1}.y, kynex_places_0{kynex_place_0 + 1}.z - kynex_places_0{1}.z), kynex_v250, fine, kynex_tool \WObj:=wobj0;
    kynex_done := 81;
    kynex_place_0 := (kynex_place_0 + 1) MOD 20;
    kynex_mark := 82;
  ENDPROC
ENDMODULE
