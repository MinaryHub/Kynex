/PROG  KYNEX_MAIN__J_P_1__100__FINE
/ATTR
OWNER		= MNEDITOR;
COMMENT		= "KYNEX";
PROG_SIZE	= 0;
CREATE		= DATE 26-01-01  TIME 00:00:00;
MODIFIED	= DATE 26-01-01  TIME 00:00:00;
FILE_NAME	= ;
VERSION		= 0;
LINE_COUNT	= 58;
MEMORY_SIZE	= 0;
PROTECT		= READ_WRITE;
TCD:  STACK_SIZE	= 0,
      TASK_PRIORITY	= 50,
      TIME_SLICE	= 0,
      BUSY_LAMP_OFF	= 0,
      ABORT_REQUEST	= 0,
      PAUSE_REQUEST	= 0;
DEFAULT_GROUP	= 1,*,*,*,*;
CONTROL_CODE	= 00000000 00000000;
/MN
   1:  !KYNEX: main??J P[1] 100% FINE ;
   2:  PR[49]=LPOS-LPOS ;
   3:  PR[49,1]=0.000 ;
   4:  PR[49,2]=0.000 ;
   5:  PR[49,3]=150.000 ;
   6:  PR[49,4]=0.000 ;
   7:  PR[49,5]=0.000 ;
   8:  PR[49,6]=0.000 ;
   9:  UTOOL[1]=PR[49] ;
  10:  UTOOL_NUM=1 ;
  11:  UFRAME_NUM=0 ;
  12:  !places of pallet ;
  13:  PR[51]=LPOS-LPOS ;
  14:  PR[51,1]=400.000 ;
  15:  PR[51,2]=0.000 ;
  16:  PR[51,3]=-200.000 ;
  17:  PR[52]=LPOS-LPOS ;
  18:  PR[52,1]=500.000 ;
  19:  PR[52,2]=0.000 ;
  20:  PR[52,3]=-200.000 ;
  21:  PR[53]=LPOS-LPOS ;
  22:  PR[53,1]=600.000 ;
  23:  PR[53,2]=0.000 ;
  24:  PR[53,3]=-200.000 ;
  25:  R[81]=0 ;
  26:  !To the start ;
  27:  R[98]=1 ;
  28:  !P1 ;
  29:J P[1] 50% FINE    ;
  30:  R[99]=1 ;
  31:  LBL[1] ;
  32:  DO[1]=ON ;
  33:  WAIT   0.50(sec) ;
  34:  R[98]=2 ;
  35:  !P2 ;
  36:L P[2] 250mm/sec CNT20    ;
  37:  R[99]=2 ;
  38:  PULSE RO[1],0.2sec ;
  39:  R[98]=3 ;
  40:  !P3 ;
  41:C P[5]    
    :  P[3] 100mm/sec FINE    ;
  42:  R[99]=3 ;
  43:  WAIT DI[4]=ON ;
  44:  R[98]=4 ;
  45:  !P4 ;
  46:  R[90]=R[81]+51 ;
  47:  PR[50]=PR[R[90]]-PR[52] ;
  48:L P[4] 250mm/sec FINE Offset,PR[50]    ;
  49:  R[99]=4 ;
  50:  R[81]=R[81]+1 ;
  51:  IF R[81]<3,JMP LBL[102] ;
  52:  R[81]=0 ;
  53:  LBL[102] ;
  54:  R[98]=5 ;
  55:  !P1 ;
  56:J P[1] 50% FINE    ;
  57:  R[99]=5 ;
  58:  JMP LBL[1] ;
/POS
P[1]{
   GP1:
	UF : 0, UT : 1,	
	J1=    11.459 deg,	J2=    17.189 deg,	J3=   -11.459 deg,
	J4=     0.000 deg,	J5=   -90.000 deg,	J6=     0.000 deg
};
P[2]{
   GP1:
	UF : 0, UT : 1,	
	J1=    17.189 deg,	J2=    22.918 deg,	J3=   -11.459 deg,
	J4=     5.730 deg,	J5=   -80.214 deg,	J6=    11.459 deg
};
P[3]{
   GP1:
	UF : 0, UT : 1,	
	J1=    28.648 deg,	J2=    22.918 deg,	J3=   -11.459 deg,
	J4=     5.730 deg,	J5=   -80.214 deg,	J6=    11.459 deg
};
P[4]{
   GP1:
	UF : 0, UT : 1,		CONFIG : 'N U T, 0, 0, 0',
	X =   611.830  mm,	Y =   165.576  mm,	Z =   127.336  mm,
	W =   174.834 deg,	P =     2.801 deg,	R =    29.579 deg
};
P[5]{
   GP1:
	UF : 0, UT : 1,	
	J1=    22.918 deg,	J2=    24.064 deg,	J3=   -12.605 deg,
	J4=     5.730 deg,	J5=   -80.214 deg,	J6=    11.459 deg
};
/END
