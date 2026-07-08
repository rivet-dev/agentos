!==========================================================================
! MMS description file for bzip2 support in UnZip 6              2008-02-16
!==========================================================================
!
! To build the LIBBZ2_NS library, edit the USER CUSTOMIZATION
! lines below to taste, then do
!	mms
! or
!	mmk
! if you use Matt's Make (free MMS-compatible make utility).
!
! In all other cases where you want to explicitly specify a makefile target,
! you have to specify your compiling environment, too. These are:
!
!	$ MMS/MACRO=(__ALPHA__=1)		! Alpha AXP, (DEC C)
!	$ MMS/MACRO=(__IA64__=1)		! IA64, (DEC C)
!	$ MMS/MACRO=(__DECC__=1)		! VAX, using DEC C
!	$ MMS/MACRO=(__FORCE_VAXC__=1)		! VAX, prefering VAXC over DECC
!	$ MMS/MACRO=(__VAXC__=1)		! VAX, where VAXC is default
!	$ MMS/MACRO=(__GNUC__=1)		! VAX, using GNU C
!

! To delete all .OBJ, .OLB, .EXE and .HLP files,
!	mms clean

## The "DO_THE_BUILD" target does no longer work with current
## releases of the MMS tool, sigh.
#DO_THE_BUILD :
#	@ decc = f$search("SYS$SYSTEM:DECC$COMPILER.EXE").nes.""
#	@ axp = (f$getsyi("HW_MODEL") .ge. 1024) .and. -
#	   (f$getsyi("HW_MODEL") .lt. 4096)
#	@ i64 = f$getsyi("HW_MODEL") .ge. 4096
#	@ macro = "/MACRO=("
#.IFDEF CCOPTS
#	@ macro = macro + """CCOPTS=$(CCOPTS)"","
#.ENDIF
#	@ if decc then macro = macro + "__DECC__=1,"
#	@ if axp then macro = macro + "__ALPHA__=1,"
#	@ if i64 then macro = macro + "__IA64__=1,"
#	@ if .not.(axp .or. i64 .or. decc) then macro = macro + "__VAXC__=1,"
#	@ macro = f$extract(0,f$length(macro)-1,macro)+ ")"
#	$(MMS)$(MMSQUALIFIERS)'macro' DEFAULT

# Define MMK architecture macros when using MMS.

.IFDEF __MMK__                  # __MMK__
.ELSE                           # __MMK__
ALPHA_X_ALPHA = 1
IA64_X_IA64 = 1
VAX_X_VAX = 1
.IFDEF $(MMS$ARCH_NAME)_X_ALPHA     # $(MMS$ARCH_NAME)_X_ALPHA
__ALPHA__ = 1
.ENDIF                              # $(MMS$ARCH_NAME)_X_ALPHA
.IFDEF $(MMS$ARCH_NAME)_X_IA64      # $(MMS$ARCH_NAME)_X_IA64
__IA64__ = 1
.ENDIF                              # $(MMS$ARCH_NAME)_X_IA64
.IFDEF $(MMS$ARCH_NAME)_X_VAX       # $(MMS$ARCH_NAME)_X_VAX
__VAX__ = 1
.ENDIF                              # $(MMS$ARCH_NAME)_X_VAX
.ENDIF                          # __MMK__

.IFDEF __ALPHA__                # __ALPHA__
DEST = ALPHA
#E = .AXP_EXE
#O = .AXP_OBJ
#A = .AXP_OLB
.ELSE                           # __ALPHA__
.IFDEF __IA64__                     # __IA64__
DEST = IA64
#E = .I64_EXE
#O = .I64_OBJ
#A = .I64_OLB
.ELSE                               # __IA64__
.IFDEF __DECC__                         # __DECC__
DEST = VAX
#E = .VAX_DECC_EXE
#O = .VAX_DECC_OBJ
#A = .VAX_DECC_OLB
.ENDIF                                  # __DECC__
.IFDEF __FORCE_VAXC__                   # __FORCE_VAXC__
__VAXC__ = 1
.ENDIF                                  # __FORCE_VAXC__
.IFDEF __VAXC__                         # __VAXC__
DEST = VAXV
#E = .VAX_VAXC_EXE
#O = .VAX_VAXC_OBJ
#A = .VAX_VAXC_OLB
.ENDIF                                  # __VAXC__
.IFDEF __GNUC__                         # __GNUC__
DEST = VAXG
#E = .VAX_GNUC_EXE
#O = .VAX_GNUC_OBJ
#A = .VAX_GNUC_OLB
.ENDIF                                  # __GNUC__
.ENDIF                              # __IA64__
.ENDIF                          # __ALPHA__
.IFDEF O                        # O
.ELSE                           # O
!If EXE and OBJ extensions aren't defined, define them
E = .EXE
O = .OBJ
A = .OLB
.ENDIF                          # O

.IFDEF SRCDIR
.ELSE
SRCDIR = []
.ENDIF
.IFDEF DSTDIR                   # DSTDIR
.ELSE                           # DSTDIR
.IFDEF DEST                       # DEST
DSTDIR = [.$(DEST)]
.ELSE                             # DEST
DSTDIR = []
.ENDIF                            # DEST
.ENDIF                          # DSTDIR

!!!!!!!!!!!!!!!!!!!!!!!!!!! USER CUSTOMIZATION !!!!!!!!!!!!!!!!!!!!!!!!!!!!
! add any common optional preprocessor flags (macros) here
! (do not forget a trailing comma!!):
COMMON_DEFS =
!!!!!!!!!!!!!!!!!!!!!!!! END OF USER CUSTOMIZATION !!!!!!!!!!!!!!!!!!!!!!!!

.IFDEF __GNUC__
CC = gcc
LIBS = ,GNU_CC:[000000]GCCLIB.OLB/LIB
.ELSE
CC = cc
LIBS =
.ENDIF

CFLAGS = /NOLIST

OPTFILE = sys$disk:[.vms]vaxcshr.opt

.IFDEF __ALPHA__                # __ALPHA__
CC_OPTIONS = /STANDARD=RELAX/PREFIX=ALL/ANSI/NAMES=(AS_IS)
CC_DEFS =
.ELSE                           # __ALPHA__
.IFDEF __IA64__                     # __IA64__
CC_OPTIONS = /STANDARD=RELAX/PREFIX=ALL/ANSI/NAMES=(AS_IS)
CC_DEFS =
.ELSE                               # __IA64__
.IFDEF __DECC__                         # __DECC__
CC_OPTIONS = /DECC/STANDARD=RELAX/PREFIX=ALL/NAMES=(AS_IS)
CC_DEFS =
.ELSE                                   # __DECC__
.IFDEF __FORCE_VAXC__                       # __FORCE_VAXC__
!Select VAXC on systems where DEC C exists
CC_OPTIONS = /VAXC
.ELSE                                       # __FORCE_VAXC__
!No flag allowed/needed on a pure VAXC system
CC_OPTIONS =
.ENDIF                                      # __FORCE_VAXC__
CC_DEFS =
.ENDIF                                  # __DECC__
.ENDIF                              # __IA64__
.ENDIF                          # __ALPHA__

!
! The .FIRST target is needed only if we're serious about building,
! and then, only if BZIP2 support was requested.
!
.IFDEF MMSTARGETS               # MMSTARGETS
.FIRST
	@ write sys$output "   Destination: $(DSTDIR)"
	@ write sys$output ""
	if ("$(DEST)" .nes. "") then -
	 if (f$search("$(DEST).DIR;1") .eqs. "") then -
	  create /directory $(DSTDIR)
.ENDIF                          # MMSTARGETS
CC_DEFS2 =
CFLAGS_INCL = /INCLUDE = []

.IFDEF __DEBUG__
CDEB = /DEBUG/NOOPTIMIZE
.ELSE
CDEB =
.ENDIF

CFLAGS_ALL  = $(CC_OPTIONS) $(CFLAGS) $(CDEB) $(CFLAGS_INCL) -
              /def=($(CC_DEFS) $(COMMON_DEFS) BZ_NO_STDIO, VMS) -
              $(CCOPTS)


OBJBZ2LIB = \
 $(DSTDIR)blocksort$(O), \
 $(DSTDIR)huffman$(O), \
 $(DSTDIR)crctable$(O), \
 $(DSTDIR)randtable$(O), \
 $(DSTDIR)compress$(O), \
 $(DSTDIR)decompress$(O), \
 $(DSTDIR)bzlib$(O)

OLBBZ2 = $(DSTDIR)LIBBZ2_NS$(A)

BZIP2_H = $(SRCDIR)bzlib.h $(SRCDIR)bzlib_private.h

!!!!!!!!!!!!!!!!!!! override default rules: !!!!!!!!!!!!!!!!!!!
.suffixes :
.suffixes : .ANL $(E) $(A) .MLB .HLB .TLB .FLB $(O) -
	    .FORM .BLI .B32 .C .c .COB -
	    .FOR .BAS .B16 .PLI .PEN .PAS .MAC .MAR .M64 .CLD .MSG .COR .DBL -
	    .RPG .SCN .IFDL .RBA .RC .RCO .RFO .RPA .SC .SCO .SFO .SPA .SPL -
	    .SQLADA .SQLMOD .RGK .RGC .MEM .RNO .HLP .RNH .L32 .REQ .R32 -
	    .L16 .R16 .TXT .H .FRM .MMS .DDL .COM .DAT .OPT .CDO .SDML .ADF -
	    .GDF .LDF .MDF .RDF .TDF

$(O)$(A) :
	If "''F$Search("$(MMS$TARGET)")'" .EQS. "" Then $(LIBR)/Create $(MMS$TARGET)
	$(LIBR)$(LIBRFLAGS) $(MMS$TARGET) $(MMS$SOURCE)

.c$(O) :
	$(CC) $(CFLAGS_ALL) /OBJ=$(MMS$TARGET) $(MMS$SOURCE)

!!!!!!!!!!!!!!!!!! here starts the bzip2 specific part !!!!!!!!!!!

DEFAULT default :	CHK_DESTDIR $(OLBBZ2)
	@	!	Do nothing.

CLEAN.COM clean.com :
	@ open/write tmp $(MMS$TARGET)
	@ write tmp "$!"
	@ write tmp "$!	Clean.com --	procedure to delete files. It always returns success"
	@ write tmp "$!			status despite any error or warnings. Also it extracts"
	@ write tmp "$!			filename from MMS ""module=file"" format."
	@ write tmp "$!"
	@ write tmp "$ on control_y then goto ctly"
	@ write tmp "$ if p1.eqs."""" then exit 1"
	@ write tmp "$ i = -1"
	@ write tmp "$scan_list:"
	@ write tmp "$	i = i+1"
	@ write tmp "$	item = f$elem(i,"","",p1)"
	@ write tmp "$	if item.eqs."""" then goto scan_list"
	@ write tmp "$	if item.eqs."","" then goto done		! End of list"
	@ write tmp "$	item = f$edit(item,""trim"")		! Clean of blanks"
	@ write tmp "$	wild = f$elem(1,""="",item)"
	@ write tmp "$	show sym wild"
	@ write tmp "$	if wild.eqs.""="" then wild = f$elem(0,""="",item)"
	@ write tmp "$	vers = f$parse(wild,,,""version"",""syntax_only"")"
	@ write tmp "$	if vers.eqs."";"" then wild = wild - "";"" + "";*"""
	@ write tmp "$scan:"
	@ write tmp "$		f = f$search(wild)"
	@ write tmp "$		if f.eqs."""" then goto scan_list"
	@ write tmp "$		on error then goto err"
	@ write tmp "$		on warning then goto warn"
	@ write tmp "$		delete/log 'f'"
	@ write tmp "$warn:"
	@ write tmp "$err:"
	@ write tmp "$		goto scan"
	@ write tmp "$done:"
	@ write tmp "$ctly:"
	@ write tmp "$	exit 1"
	@ close tmp

CLEAN clean : clean.com
	@clean "$(OBJBZ2LIB)"
	@clean "$(OLBBZ2)"
	@- delete/noconfirm/nolog clean.com;*
        @- if ("$(DEST).dir" .nes. "") then -
	   if (f$search("$(DEST).dir") .nes. "") then -
	    set protect=w:d $(DEST).dir;*
        @- if ("$(DEST).dir" .nes. "") then -
	   if (f$search("$(DEST).dir") .nes. "") then -
	    delete/noconfirm $(DEST).dir;*

CHK_DESTDIR chk_destdir :
	@ If ("$(DEST)" .NES. "") Then -
	   If "''F$Search("$(DEST).DIR;1")'" .EQS. "" Then -
	    Create /directory $(DSTDIR)

$(OLBBZ2)	: $(OBJBZ2LIB)
	If "''F$Search("$(MMS$TARGET)")'" .EQS. "" Then $(LIBR)/Create $(MMS$TARGET)
	$(LIBR)$(LIBRFLAGS) $(MMS$TARGET) $(MMS$CHANGED_LIST)
	@ write sys$output "$(MMS$TARGET) updated."

$(OBJBZ2LIB)	: $(BZIP2_H)

$(DSTDIR)blocksort$(O)	: $(SRCDIR)blocksort.c
$(DSTDIR)huffman$(O)	: $(SRCDIR)huffman.c
$(DSTDIR)crctable$(O)	: $(SRCDIR)crctable.c
$(DSTDIR)randtable$(O)	: $(SRCDIR)randtable.c
$(DSTDIR)compress$(O)	: $(SRCDIR)compress.c
$(DSTDIR)decompress$(O)	: $(SRCDIR)decompress.c
$(DSTDIR)bzlib$(O)	: $(SRCDIR)bzlib.c
