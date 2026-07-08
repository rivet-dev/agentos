# WMAKE makefile for Windows 9x and Windows NT (Intel only)
# using Watcom C/C++ v11.0+, by Paul Kienitz, last revised 18 Jan 2009.
# Makes UnZip.exe, fUnZip.exe, and UnZipSFX.exe.
#
# Invoke from UnZip source dir with "WMAKE -F WIN32\MAKEFILE.WAT [targets]"
# To build with debug info use "WMAKE DEBUG=1 ..."
# To build with no assembly modules use "WMAKE NOASM=1 ..."
# To omit unshrinking support use "WMAKE NO_LZW=1 ..."
# To support unreducing, get the real unreduce.c and go "WMAKE OFFEND_RMS=1 ..."
# To use Info-Zip's generic timezone functions use "WMAKE USE_IZTIMEZONE=1 ..."
# To include support for bzip2 decompression, get the bzip2 source distribution
#  into the bzip2/ subfolder and start compilation with "WMAKE USEBZ2=1 ..."
#
# Other options to be fed to the compiler can be specified in an environment
# variable called LOCAL_UNZIP.

variation = $(%LOCAL_UNZIP)

# Stifle annoying "Delete this file?" questions when errors occur:
.ERASE

.EXTENSIONS:
.EXTENSIONS: .exe .obj .obx .c .h .asm

# We maintain multiple sets of object files in different directories so that
# we can compile msdos, dos/4gw or pmode/w, and win32 versions of UnZip without
# their object files interacting.  The following var must be a directory name
# ending with a backslash.  All object file names must include this macro
# at the beginning, for example "$(O)foo.obj".

!ifdef DEBUG
OBDIR = od32w
!else
OBDIR = ob32w
!endif
O = $(OBDIR)\   # comment here so backslash won't continue the line

!ifdef NO_LZW
cvars = $+$(cvars)$- -DLZW_CLEAN
avars = $+$(avars)$- -DLZW_CLEAN
# "$+$(foo)$-" means expand foo as it has been defined up to now; normally,
# this Make defers inner expansion until the outer macro is expanded.
!endif
!ifdef OFFEND_RMS
cvars = $+$(cvars)$- -DUSE_SMITH_CODE
avars = $+$(avars)$- -DUSE_SMITH_CODE
!endif

IZ_BZIP2 = bzip2
!ifdef USEBZ2
cvars = $+$(cvars)$- -DUSE_BZIP2 -I$(IZ_BZIP2)
BZIPLIB = $(IZ_BZIP2)/$(OBDIR)/bz2.lib
BZ2LNKLIB = lib {$(BZIPLIB)}
!else
BZIPLIB =
BZ2LNKLIB =
!endif

!ifndef USE_IZTIMEZONE
#default: do not use the IZ timezone replacement
USE_IZTIMEZONE=0
!endif

# The assembly hot-spot code in crc_i386.asm is optional.  This section
# controls its usage.

!ifdef NOASM
crcaob =
!else   # !NOASM
cvars = $+$(cvars)$- -DASM_CRC
crcaob = $(O)crc_i386.obj
!endif

!if $(USE_IZTIMEZONE) != 0
cvars = $+$(cvars)$- -DW32_USE_IZ_TIMEZONE
TIMEZONE_OBJU = $(O)timezone.obj
TIMEZONE_OBJX = $(O)timezone.obx
TIMEZONE_OBJD = $(O)timezonl.obj
TIMEZONE_OBLX = $(O)timezolx.obj
TIMEZONE_OBJB = $(O)timezonb.obj
!else
TIMEZONE_OBJU =
TIMEZONE_OBJX =
TIMEZONE_OBJD =
TIMEZONE_OBLX =
TIMEZONE_OBJB =
!endif

# Our object files.  OBJU is for UnZip, OBJX for UnZipSFX, OBJF for fUnZip:

OBJU1 = $(O)unzip.obj $(O)crc32.obj $(crcaob) $(O)crypt.obj $(O)envargs.obj
OBJU2 = $(O)explode.obj $(O)extract.obj $(O)fileio.obj $(O)globals.obj
OBJU3 = $(O)inflate.obj $(O)list.obj $(O)match.obj $(O)process.obj
OBJU4 = $(O)ubz2err.obj $(O)ttyio.obj $(O)unreduce.obj $(O)unshrink.obj
OBJU5 = $(O)zipinfo.obj
OBJUS = $(O)win32.obj $(O)win32i64.obj $(O)nt.obj $(TIMEZONE_OBJU)
OBJU  = $(OBJU1) $(OBJU2) $(OBJU3) $(OBJU4) $(OBJU5) $(OBJUS)

OBJX1 = $(O)unzip.obx $(O)crc32.obx $(crcaob) $(O)crypt.obx $(O)extract.obx
OBJX2 = $(O)fileio.obx $(O)globals.obx $(O)inflate.obx $(O)match.obx
OBJX3 = $(O)process.obx $(O)ttyio.obx $(O)ubz2err.obx
OBJXS = $(O)win32.obx $(O)win32i64.obx $(O)nt.obx $(TIMEZONE_OBJX)
OBJX  = $(OBJX1) $(OBJX2) $(OBJX3) $(OBJXS)

OBJF1 = $(O)funzip.obj $(O)crc32f.obj $(crcaob) $(O)cryptf.obj
OBJF2 = $(O)globalsf.obj $(O)inflatef.obj $(O)ttyiof.obj
OBJFS = $(O)win32f.obj $(O)win32i64f.obj
OBJF  = $(OBJF1) $(OBJF2) $(OBJFS)

OBJD1 = $(O)api.obj $(O)crc32l.obj $(crcaob) $(O)cryptl.obj $(O)explodel.obj
OBJD2 = $(O)extractl.obj $(O)fileiol.obj $(O)globalsl.obj $(O)inflatel.obj
OBJD3 = $(O)listl.obj $(O)matchl.obj $(O)processl.obj $(O)ubz2errl.obj
OBJD4 = $(O)unreducl.obj $(O)unshrnkl.obj $(O)zipinfol.obj
OBJDS = $(O)win32l.obj $(O)win32i64l.obj $(O)ntl.obj $(O)windll.obj $(TIMEZONE_OBJD)
OBJD  = $(OBJD1) $(OBJD2) $(OBJD3) $(OBJD4) $(OBJDS)

UNZIP_H = unzip.h unzpriv.h globals.h win32/w32cfg.h
WINDLL_H = windll/windll.h windll/decs.h windll/structs.h
WINDLL_DEF = windll/windll32.def
WINDLL_IMP_H = windll/decs.h windll/structs.h

# Now we have to pick out the proper compiler and options for it.

cc     = wcc386
link   = wlink
asm    = wasm
rc     = wrc
# Use Pentium Pro timings, register args, static strings in code, high strictness:
cflags = -bt=NT -6r -zt -zq -wx
aflags = -bt=NT -mf -3 -zq
rflags = -bt=NT
lflags = sys NT
lflags_dll = sys NT_DLL
cvars  = $+$(cvars)$- -DWIN32 $(variation)
avars  = $+$(avars)$- $(variation)

# Specify optimizations, or a nonoptimized debugging version:

!ifdef DEBUG
cdebug = -od -d2
cdebux = -od -d2
ldebug = d w all op symf
!else
cdebug = -s -obhikl+rt -oe=100 -zp8
cdebux = -s -obhiklrs
# -oa helps slightly but might be dangerous.
ldebug = op el
!endif

CFLAGS_FU = $(cdebug) $(cflags) $(cvars) -DFUNZIP
CFLAGS_DL = $(cdebug) $(cflags) $(cvars) -bd -bm -DWINDLL -DDLL

# How to compile sources:
.c.obx:
	$(cc) $(cdebux) $(cflags) $(cvars) -DSFX $[@ -fo=$@

.c.obj:
	$(cc) $(cdebug) $(cflags) $(cvars) $[@ -fo=$@

# Here we go!  By default, make all targets:
all: UnZip.exe fUnZip.exe UnZipSFX.exe

# Convenient shorthand options for single targets:
u:   UnZip.exe     .SYMBOLIC
f:   fUnZip.exe    .SYMBOLIC
x:   UnZipSFX.exe  .SYMBOLIC
d:   UnZip32.dll   .SYMBOLIC

UnZip.exe:	$(OBDIR) $(OBJU) $(BZIPLIB) $(O)winapp.res
	$(link) $(lflags) $(ldebug) name $@ file {$(OBJU)} $(BZ2LNKLIB)

UnZipSFX.exe:	$(OBDIR) $(OBJX) $(BZIPLIB)
	$(link) $(lflags) $(ldebug) name $@ file {$(OBJX)} $(BZ2LNKLIB)

fUnZip.exe:	$(OBDIR) $(OBJF)
	$(link) $(lflags) $(ldebug) name $@ file {$(OBJF)}

UnZip32.dll:	$(OBDIR) $(OBJD) $(BZIPLIB) $(O)windll.res
	$(link) $(lflags_dll) $(ldebug) name $@ file {$(OBJD)} $(BZ2LNKLIB)
	$(rc) $(rflags) $(O)windll.res $@

uzexampl.exe:	$(OBDIR) $(O)uzexampl.obj
	$(link) $(lflags) $(ldebug) name $@ file {$(O)uzexampl.obj} lib version.lib

# Source dependencies:

#       generic (UnZip, fUnZip):

$(O)crc32.obj:    crc32.c $(UNZIP_H) zip.h crc32.h
$(O)crypt.obj:    crypt.c $(UNZIP_H) zip.h crypt.h crc32.h ttyio.h
$(O)envargs.obj:  envargs.c $(UNZIP_H)
$(O)explode.obj:  explode.c $(UNZIP_H)
$(O)extract.obj:  extract.c $(UNZIP_H) crc32.h crypt.h
$(O)fileio.obj:   fileio.c $(UNZIP_H) crc32.h crypt.h ttyio.h ebcdic.h
$(O)funzip.obj:   funzip.c $(UNZIP_H) crc32.h crypt.h ttyio.h
$(O)globals.obj:  globals.c $(UNZIP_H)
$(O)inflate.obj:  inflate.c inflate.h $(UNZIP_H)
$(O)list.obj:     list.c $(UNZIP_H)
$(O)match.obj:    match.c $(UNZIP_H)
$(O)process.obj:  process.c $(UNZIP_H) crc32.h
$(O)timezone.obj: timezone.c $(UNZIP_H) zip.h timezone.h
$(O)ttyio.obj:    ttyio.c $(UNZIP_H) zip.h crypt.h ttyio.h
$(O)ubz2err.obj:  ubz2err.c $(UNZIP_H)
$(O)unreduce.obj: unreduce.c $(UNZIP_H)
$(O)unshrink.obj: unshrink.c $(UNZIP_H)
$(O)unzip.obj:    unzip.c $(UNZIP_H) crypt.h unzvers.h consts.h
$(O)zipinfo.obj:  zipinfo.c $(UNZIP_H)

#       UnZipSFX variants:

$(O)crc32.obx:    crc32.c $(UNZIP_H) zip.h crc32.h
$(O)crypt.obx:    crypt.c $(UNZIP_H) zip.h crypt.h crc32.h ttyio.h
$(O)extract.obx:  extract.c $(UNZIP_H) crc32.h crypt.h
$(O)fileio.obx:   fileio.c $(UNZIP_H) crc32.h crypt.h ttyio.h ebcdic.h
$(O)globals.obx:  globals.c $(UNZIP_H)
$(O)inflate.obx:  inflate.c inflate.h $(UNZIP_H)
$(O)match.obx:    match.c $(UNZIP_H)
$(O)process.obx:  process.c $(UNZIP_H) crc32.h
$(O)timezone.obx: timezone.c $(UNZIP_H) zip.h timezone.h
$(O)ttyio.obx:    ttyio.c $(UNZIP_H) zip.h crypt.h ttyio.h
$(O)ubz2err.obx:  ubz2err.c $(UNZIP_H)
$(O)unzip.obx:    unzip.c $(UNZIP_H) crypt.h unzvers.h consts.h

# Special case object files:

$(O)win32.obj:    win32\win32.c $(UNZIP_H) win32\nt.h
	$(cc) $(cdebug) $(cflags) $(cvars) win32\win32.c -fo=$@

$(O)win32.obx:    win32\win32.c $(UNZIP_H) win32\nt.h
	$(cc) $(cdebux) $(cflags) $(cvars) -DSFX win32\win32.c -fo=$@

$(O)win32i64.obj: win32\win32i64.c $(UNZIP_H)
	$(cc) $(cdebug) $(cflags) $(cvars) win32\win32i64.c -fo=$@

$(O)win32i64.obx: win32\win32i64.c $(UNZIP_H)
	$(cc) $(cdebux) $(cflags) $(cvars) -DSFX win32\win32i64.c -fo=$@

$(O)nt.obj:    win32\nt.c $(UNZIP_H) win32\nt.h
	$(cc) $(cdebug) $(cflags) $(cvars) win32\nt.c -fo=$@

$(O)nt.obx:    win32\nt.c $(UNZIP_H) win32\nt.h
	$(cc) $(cdebux) $(cflags) $(cvars) -DSFX win32\nt.c -fo=$@

$(O)crc_i386.obj: win32\crc_i386.asm
	$(asm) $(aflags) $(avars) win32\crc_i386.asm -fo=$@

$(O)winapp.res:	win32\winapp.rc unzvers.h
	$(rc) $(rflags) /r /fo=$@ /dWIN32 win32\winapp.rc

# Variant object files for fUnZip:

$(O)crc32f.obj:   crc32.c $(UNZIP_H) zip.h crc32.h
	$(cc) $(CFLAGS_FU) crc32.c -fo=$@

$(O)cryptf.obj:   crypt.c $(UNZIP_H) zip.h crypt.h ttyio.h
	$(cc) $(CFLAGS_FU) crypt.c -fo=$@

$(O)globalsf.obj: globals.c $(UNZIP_H)
	$(cc) $(CFLAGS_FU) globals.c -fo=$@

$(O)inflatef.obj: inflate.c inflate.h $(UNZIP_H) crypt.h
	$(cc) $(CFLAGS_FU) inflate.c -fo=$@

$(O)ttyiof.obj:   ttyio.c $(UNZIP_H) zip.h crypt.h ttyio.h
	$(cc) $(CFLAGS_FU) ttyio.c -fo=$@

$(O)win32f.obj:   win32\win32.c $(UNZIP_H)
	$(cc) $(CFLAGS_FU) win32\win32.c -fo=$@

$(O)win32i64f.obj: win32\win32i64.c $(UNZIP_H)
	$(cc) $(CFLAGS_FU) win32\win32i64.c -fo=$@

# DLL compilation section
$(O)api.obj:      api.c $(UNZIP_H) $(WINDLL_H) unzvers.h
	$(cc) $(CFLAGS_DL) api.c -fo=$@
$(O)crc32l.obj:   crc32.c $(UNZIP_H) zip.h crc32.h
	$(cc) $(CFLAGS_DL) crc32.c -fo=$@
$(O)cryptl.obj:   crypt.c $(UNZIP_H) zip.h crypt.h crc32.h ttyio.h
	$(cc) $(CFLAGS_DL) crypt.c -fo=$@
$(O)explodel.obj: explode.c $(UNZIP_H)
	$(cc) $(CFLAGS_DL) explode.c -fo=$@
$(O)extractl.obj: extract.c $(UNZIP_H) $(WINDLL_H) crc32.h crypt.h
	$(cc) $(CFLAGS_DL) extract.c -fo=$@
$(O)fileiol.obj:  fileio.c $(UNZIP_H) $(WINDLL_H) crc32.h crypt.h ttyio.h ebcdic.h
	$(cc) $(CFLAGS_DL) fileio.c -fo=$@
$(O)globalsl.obj: globals.c $(UNZIP_H)
	$(cc) $(CFLAGS_DL) globals.c -fo=$@
$(O)inflatel.obj: inflate.c inflate.h $(UNZIP_H)
	$(cc) $(CFLAGS_DL) inflate.c -fo=$@
$(O)listl.obj:	  list.c $(UNZIP_H) $(WINDLL_H)
	$(cc) $(CFLAGS_DL) list.c -fo=$@
$(O)matchl.obj:	  match.c $(UNZIP_H)
	$(cc) $(CFLAGS_DL) match.c -fo=$@
$(O)processl.obj: process.c $(UNZIP_H) $(WINDLL_H) crc32.h
	$(cc) $(CFLAGS_DL) process.c -fo=$@
$(O)timezonl.obj: timezone.c $(UNZIP_H) zip.h timezone.h
	$(cc) $(CFLAGS_DL) timezone.c -fo=$@
$(O)ubz2errl.obj: ubz2err.c $(UNZIP_H)
	$(cc) $(CFLAGS_DL) ubz2err.c -fo=$@
$(O)unreducl.obj: unreduce.c $(UNZIP_H)
	$(cc) $(CFLAGS_DL) unreduce.c -fo=$@
$(O)unshrnkl.obj: unshrink.c $(UNZIP_H)
	$(cc) $(CFLAGS_DL) unshrink.c -fo=$@
$(O)zipinfol.obj: zipinfo.c $(UNZIP_H)
	$(cc) $(CFLAGS_DL) zipinfo.c -fo=$@

$(O)win32l.obj:	  win32\win32.c $(UNZIP_H) win32\nt.h
	$(cc) $(CFLAGS_DL) -I. win32\win32.c -fo=$@

$(O)win32i64l.obj: win32\win32i64.c $(UNZIP_H)
	$(cc) $(CFLAGS_DL) -I. win32\win32i64.c -fo=$@

$(O)ntl.obj:      win32\nt.c $(UNZIP_H) win32\nt.h
	$(cc) $(CFLAGS_DL) -I. win32\nt.c -fo=$@

$(O)windll.obj:   windll\windll.c $(UNZIP_H) $(WINDLL_H)
$(O)windll.obj:   crypt.h unzvers.h consts.h
	$(cc) $(CFLAGS_DL) -I. windll\windll.c -fo=$@

$(O)windll.res:	windll\windll.rc unzvers.h
	$(rc) $(rflags) /r /fo=$@ /i=windll /dWIN32 windll\windll.rc

# Windll command line example:
$(O)uzexampl.obj: windll/uzexampl.c windll/uzexampl.h
	$(cc) $(cdebug) $(cflags) $(cvars) windll\uzexampl.c -fo=$@

# The bzip2 (de)compression library for BZIP2 support:
$(IZ_BZIP2)/$(OBDIR)/bz2.lib : .ALWAYS .RECHECK
	$(MAKE) -h -f $(IZ_BZIP2)/makbz2iz.wat CC=$(cc) AR=lib386 &
	CFLSYS="$(cdebug) $(cflags)" &
	BZROOTDIR=$(IZ_BZIP2) BZOBDIR=$(IZ_BZIP2)/$(OBDIR)

# Creation of subdirectory for intermediate files
$(OBDIR):
	-mkdir $@

# Unwanted file removal:

clean_bz2_lib: .SYMBOLIC
	-$(MAKE) -h -f $(IZ_BZIP2)/makbz2iz.wat &
	BZROOTDIR=$(IZ_BZIP2) BZOBDIR=$(IZ_BZIP2)\$(OBDIR) clean

clean:     clean_bz2_lib  .SYMBOLIC
	del $(O)*.ob?
	del $(O)*.res

cleaner:   clean  .SYMBOLIC
	del UnZip.exe
	del fUnZip.exe
	del UnZipSFX.exe
	del UnZip32.dll
	del uzexampl.exe
