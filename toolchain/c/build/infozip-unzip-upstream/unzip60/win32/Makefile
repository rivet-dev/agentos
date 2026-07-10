# NMAKE Makefile for Windows NT/2K/XP/... and Windows 95/98/Me
#   D. Feinleib 7/92 <t-davefe@microsoft.com>
#   H. Gessau 9/93 <henryg@kullmar.kullmar.se>
#   J. Lee 8/95 (johnnyl@microsoft.com)
#   C. Spieler 03/99
#
# Last revised:  06 Jan 2009
#
# Tested with VC++ 2.0 for NT for MIPS and Alpha, Visual C++ 2.2 for Intel CPUs
# Revision tested with VC++ 5.0, 6.0, 8.0 and 9.0 for Intel CPUs
#
# Option switches
# (specify OPTFLAG as "OPTFLAG={1|0}" on the nmake command line)
#   USEBZ2DLL=1         : bzip2 support, link against externally supplied dll
#   USEBZ2STAT=1        : bzip2 support by static library created during build
#   USEBZ2=1            : alias for USEBZ2STAT=1
#
#   USEZLIBDLL=1        : replace built-in inflate by external zlib dll
#   USEZLIBSTAT=1       : replace built-in inflate by external zlib static lib
#   USEZLIB=1           : alias for USEZLIBSTAT=1
#
#   USE_DLLRUNTIME      : link executables against C runtime dll
#                         (default: executables are linked against static lib)
#                         USEBZ2DLL or USEZLIBDLL imply USE_DLLRUNTIME !
#   DLLSTANDALONE       : unzip32.dll is linked against static MT runtime lib
#                         (default: unzip32.dll linked against C runtime dll)
#
#   USE_UTF8            : =0 no unicode names support
#                         =1 force recognition of UTF8 extra fields and
#                            "UTF8 name" attrib (default)
#
#   USEASM              : use assembler code for crc32 function (default)
#   NOASM               : use C implementation for crc32 function
#   USEMASM             : use MS macro assembler (MASM) to assemble crc32 code
#   USEML               : use MS macro assembler (ML driver) to create crc32
#                         (default: use C inline assembler code for crc32)
#
#   NOCRC_OPT=1         : disable "unfolding CRC tables" optimization
#
#   USEIZTIMEZONE       : =0 (default) timezone setup by built-in C RTL
#                         =1 apply InfoZip replacement code for timezone setup
#                            (recognizes extended syntax when timezone setup
#                            is overridden by environment variable)

# Nmake macros for building Win32 applications
# To build with debug info use 'nmake debug=1'
!IFNDEF debug
NODEBUG=1
!ENDIF

!ifdef NOASM
APPLY_ASMCRC=0
!else
!ifdef USEASM
APPLY_ASMCRC=1
!endif
!endif

# Options to decide whether using zlib as decompression library.
# Specify USEZLIBDLL or USEZLIBSTAT to replace UnZip's built-in inflation code
# by the inflate service of the zlib library.
# (The option USEZLIB is currently set as a synonym for USEZLIBSTAT.)
!if defined(USEZLIBSTAT) || defined(USEZLIBDLL)
USEZLIB=1
!endif

!ifdef USEZLIB
!if defined(USEZLIBSTAT) && defined(USEZLIBDLL)
!ERROR Conflicting options USEZLIBSTAT and USEZLIBDLL requested!
!endif
# Set static zlib linkage as default in case nothing is specified.
!if !defined(USEZLIBSTAT) && !defined(USEZLIBDLL)
USEZLIBSTAT=1
!endif
!undef USEASM
APPLY_ASMCRC=0
NOCRC_OPT=1
ZLIBLIB=zlib.lib
ZLIBLIBDLL=zdll.lib
!else # !USEZLIB
ZLIBLIB=
ZLIBLIBDLL=
!endif # ?USEZLIB


# Options to include optional support for bzip2 decompression
!if defined(USEBZ2STAT) || defined(USEBZ2DLL)
USEBZ2=1
!endif

IZ_BZIP2 = bzip2
# Subfolder containing bz2 objects/lib for UnZip program target(s).
OBEXE = obexe
# Subfolder containing bz2 objects/lib for UnZip DLL target(s).
OBDLL = obdll
!ifdef USEBZ2
!if defined(USEBZ2STAT) && defined(USEBZ2DLL)
!ERROR Conflicting options USEBZ2STAT and USEBZ2DLL requested!
!endif
INC_BZ2LIB=-I$(IZ_BZIP2)
# Set static bzip2 linkage as default in case nothing is specified.
!if !defined(USEBZ2STAT) && !defined(USEBZ2DLL)
USEBZ2STAT=1
!endif
!IFNDEF debug
!ifdef USEBZ2STAT
BZIPLIBEXE=$(IZ_BZIP2)/$(OBEXE)/bz2.lib
BZIPLIBDLL=$(IZ_BZIP2)/$(OBDLL)/bz2.lib
!else
BZIPLIBEXE=bz2.lib
BZIPLIBDLL=bz2.lib
!endif
!ELSE
BZIPLIBEXE=bz2d.lib
BZIPLIBDLL=bz2d.lib
!ENDIF
!else # !USEBZ2
INC_BZ2LIB=
BZIPLIBEXE=
BZIPLIBDLL=
!endif # ?USEBZ2

!ifndef APPLY_ASMCRC
#default ASM CRC code (inline asm in C source) for now...
APPLY_ASMCRC=1
!endif

!ifndef USE_IZTIMEZONE
#default: do not use the IZ timezone replacement
USE_IZTIMEZONE=0
!endif

!ifndef USE_UTF8
#default for now: include support for UTF8 paths
USE_UTF8=1
!endif

!IF "$(CPU)" == ""  &&  "$(PROCESSOR_ARCHITECTURE)" == "x86"
CPU = i386
!ENDIF
!IF "$(CPU)" == ""  &&  "$(PROCESSOR_ARCHITECTURE)" == "MIPS"
CPU = $(PROCESSOR_ARCHITECTURE)
!ENDIF
!IF "$(CPU)" == ""  &&  "$(PROCESSOR_ARCHITECTURE)" == "ALPHA"
CPU = $(PROCESSOR_ARCHITECTURE)
!ENDIF
!IF "$(CPU)" == ""  &&  "$(PROCESSOR_ARCHITECTURE)" == "PPC"
CPU = $(PROCESSOR_ARCHITECTURE)
!ENDIF
!IF "$(CPU)" == ""
CPU = i386
!ENDIF

# special subdirectory for the static library binaries, to separate them
# from other binaries (including dll import library file)
OBDIR = ow32lib
OB = $(OBDIR)/

# object files
OBJU1 = unzip.obj crc32.obj $(CRCA_O) crypt.obj envargs.obj explode.obj
OBJU2 = extract.obj fileio.obj globals.obj inflate.obj list.obj match.obj
OBJU3 = process.obj ttyio.obj ubz2err.obj unreduce.obj unshrink.obj zipinfo.obj
OBJUS = win32.obj win32i64.obj nt.obj $(TIMEZONE_OBJU)
OBJU  = $(OBJU1) $(OBJU2) $(OBJU3) $(OBJUS)
OBJX1 = unzipsfx.obj crc32_.obj $(CRCA_OX) crypt_.obj
OBJX2 = extract_.obj fileio_.obj globals_.obj inflate_.obj
OBJX3 = match_.obj process_.obj ttyio_.obj ubz2err_.obj
OBJXS = win32_.obj win32i64_.obj nt_.obj $(TIMEZONE_OBJX)
OBJX  = $(OBJX1) $(OBJX2) $(OBJX3) $(OBJXS)
OBJF1 = funzip.obj crc32f.obj $(CRCA_OF) cryptf.obj globalsf.obj
OBJF2 = inflatef.obj ttyiof.obj
OBJFS = win32f.obj win32i64f.obj
OBJF  = $(OBJF1) $(OBJF2) $(OBJFS)
OBJD1 = api.obj crc32l.obj $(CRCA_OL) cryptl.obj explodel.obj
OBJD2 = extractl.obj fileiol.obj globalsl.obj inflatel.obj
OBJD3 = listl.obj matchl.obj processl.obj
OBJD4 = ubz2errl.obj unreducl.obj unshrnkl.obj zipinfol.obj
OBJDS = win32l.obj win32i64l.obj ntl.obj windll.obj $(TIMEZONE_OBJD)
OBJD  = $(OBJD1) $(OBJD2) $(OBJD3) $(OBJD4) $(OBJDS)
OBLX1 = apig.obj crc32g.obj $(CRCA_OG) cryptg.obj
OBLX2 = extractg.obj fileiog.obj globalsg.obj inflateg.obj
OBLX3 = matchg.obj processg.obj
OBLXS = win32g.obj win32i64g.obj ntg.obj windllg.obj $(TIMEZONE_OBLX)
OBLX  = $(OBLX1) $(OBLX2) $(OBLX3) $(OBLXS)
OBGX  = sfxwiz.obj
OBJB1 = $(OB)apib.obj $(OB)crc32b.obj $(CRCA_OB) $(OB)cryptb.obj
OBJB2 = $(OB)explodeb.obj $(OB)extractb.obj $(OB)fileiob.obj $(OB)globalsb.obj
OBJB3 = $(OB)inflateb.obj $(OB)listb.obj $(OB)matchb.obj $(OB)processb.obj
OBJB4 = $(OB)ubz2errb.obj $(OB)unreducb.obj $(OB)unshrnkb.obj $(OB)zipinfob.obj
OBJBS = $(OB)win32b.obj $(OB)win32i64b.obj $(OB)ntb.obj $(OB)windllb.obj $(TIMEZONE_OBJB)
OBJB  = $(OBJB1) $(OBJB2) $(OBJB3) $(OBJB4) $(OBJBS)

UNZIP_H = unzip.h unzpriv.h globals.h win32/w32cfg.h
WINDLL_H = windll/windll.h windll/decs.h windll/structs.h
WINDLL_DEF = windll/windll32.def
WINDLL_IMP_H = windll/decs.h windll/structs.h

!if $(APPLY_ASMCRC) != 0
CFVARS_ASM = -DASM_CRC
!else
CFVARS_ASM = -DNO_ASM
!endif

!if $(USE_IZTIMEZONE) != 0
LOCAL_UNZIP = -DW32_USE_IZ_TIMEZONE $(LOCAL_UNZIP)
!endif

!if $(USE_UTF8) != 0
LOCAL_UNZIP = -DUNICODE_SUPPORT $(LOCAL_UNZIP)
!else
LOCAL_UNZIP = -DNO_UNICODE_SUPPORT $(LOCAL_UNZIP)
!endif

!ifndef NOCRC_OPT
LOCAL_UNZIP = -DIZ_CRCOPTIM_UNFOLDTBL $(LOCAL_UNZIP)
!endif

!ifdef USEZLIB
LOCAL_UNZIP = -DUSE_ZLIB $(LOCAL_UNZIP)
!endif
!ifdef USEBZ2
LOCAL_UNZIP = -DUSE_BZIP2 $(LOCAL_UNZIP)
!endif

#CF_LOC = -nologo -J -D_MBCS $(CFVARS_ASM) -DREENTRANT
CF_LOC = -nologo -J -D_MBCS $(CFVARS_ASM) $(LOCAL_UNZIP)

!if $(APPLY_ASMCRC) != 0
CRCA_O = crc_i386.obj
CRCA_OX = $(CRCA_O)
CRCA_OF = $(CRCA_O)
CRCA_OL = crci386l.obj
CRCA_OG = crci386g.obj
CRCA_OB = $(OB)crci386b.obj
!else
CRCA_O =
CRCA_OX =
CRCA_OF =
CRCA_OL =
CRCA_OG =
CRCA_OB =
!endif

!if $(USE_IZTIMEZONE) != 0
TIMEZONE_OBJU = timezone.obj
TIMEZONE_OBJX = timezon_.obj
TIMEZONE_OBJD = timezonl.obj
TIMEZONE_OBLX = timezolx.obj
TIMEZONE_OBJB = $(OB)timezonb.obj
!else
TIMEZONE_OBJU =
TIMEZONE_OBJX =
TIMEZONE_OBJD =
TIMEZONE_OBLX =
TIMEZONE_OBJB =
!endif

# binary declarations common to all platforms
cc     = cl
rc     = rc
link   = link
implib = lib
hc     = hcrtf -xn

# declarations common to all compiler options
cflags = -c -W3 -DCRTAPI1=_cdecl -DCRTAPI2=_cdecl -nologo
!IF "$(CPU)" == "i386"
cflags = $(cflags) -D_X86_=1
!ELSE
!IF "$(CPU)" == "ALPHA"
cflags = $(cflags) -D_ALPHA_=1
!ENDIF
!ENDIF

# for Windows applications that use the C Run-Time libraries
cvars      = -DWIN32 -D_WIN32
cvarsmt    = $(cvars) -MT
cvarsdll   = $(cvars) -MD

## The CRT library, depending on Compiler version and target configuration.
## This Makefile version has been changed to rely on the 'default library'
## notations in the compiled object files, to work around differences between
## MS Visual C++ releases concerning libraries support.
#libc = libc.lib oldnames.lib
#libcmt = libcmt.lib oldnames.lib
#libcdll = msvcrt.lib oldnames.lib

# optional profiling and tuning libraries
!IFDEF PROFILE
optlibs =  cap.lib
!ELSE
!IFDEF TUNE
optlibs = wst.lib
!ELSE
optlibs =
!ENDIF
!ENDIF

# USER32.LIB is needed for OemToChar and related routines
baselibs = kernel32.lib $(optlibs) advapi32.lib user32.lib
winlibs  = $(baselibs) gdi32.lib comdlg32.lib winspool.lib
conlibs = $(baselibs)
conlibsmt = $(baselibs)
conlibsdll = $(baselibs)
guilibs = $(winlibs)
guilibsdll = $(winlibs)

# Some optimization (if not debugging)
!IFDEF NODEBUG
cdebug = -O2
cdbgsz = -O1
!ELSE
!IFDEF PROFILE
cdebug = -Gh -Zd -Ox
!ELSE
!IFDEF TUNE
cdebug = -Gh -Zd -Ox
!ELSE
cdebug = -Z7 -Od
!ENDIF
!ENDIF
cdbgsz = $(cdebug)
!ENDIF

# declarations common to all linker options
lflags  = /INCREMENTAL:NO /PDB:NONE /NOLOGO

# for Windows applications that use the C Run-Time libraries
conlflags = $(lflags) /subsystem:console
guilflags = $(lflags) /subsystem:windows
dlllflags = $(lflags) /DLL

# -------------------------------------------------------------------------
# Target Module Dependent Link Debug Flags - must be specified after $(link)
#
# These switches allow the inclusion of the necessary symbolic information
# for source level debugging with WinDebug, profiling and/or performance
# tuning.
#
# Note: Debug switches are on by default.
# -------------------------------------------------------------------------
!IFDEF NODEBUG
ldebug = /RELEASE
!ELSE
!IFDEF PROFILE
ldebug = -debug:mapped,partial -debugtype:coff
!ELSE
!IFDEF TUNE
ldebug = -debug:mapped,partial -debugtype:coff
!ELSE
ldebug = -debug:full -debugtype:cv
!ENDIF
!ENDIF
!ENDIF

!ifdef PROFILEINFO
ldebug = $(ldebug) /PROFILE
!endif

!ifdef SYM
ldebug = $(ldebug) /DEBUG:FULL /DEBUGTYPE:CV
cdebug = $(cdebug) -Zi
cdbgsz = $(cdbgsz) -Zi
!endif

# When using zlib and/or bzip2 external decompression code DLLs, we want
# to link against the dll version of the C runtime library.
!IF defined(USEZLIBDLL) || defined(USEBZ2DLL) || defined(USE_DLLRUNTIME)
LINK_CRTDLL=1
!UNDEF DLLSTANDALONE
!ELSE
LINK_CRTDLL=0
!ENDIF

!IF $(LINK_CRTDLL) == 1
# Using zlib DLL as decompression core (and/or bzip2 decompression dll)
# implies linking all executables against the DLL version of the C RTL.
cvars_iz = $(cvarsdll) $(CF_LOC)
cvars_izdll = $(cvarsdll) $(CF_LOC)
conlibs_iz = $(conlibsdll) $(ZLIBLIBDLL) $(BZIPLIBEXE)
conlibs_izdll = $(conlibsdll) $(ZLIBLIBDLL) $(BZIPLIBDLL)
guilibs_iz = $(guilibsdll) $(ZLIBLIBDLL) $(BZIPLIBEXE)
!ELSE
# With the default setup (using Info-ZIP's own inflate code and/or static
# external decompression code), the executables are linked against the
# single-thread static C-RTL library to get self-contained programs.
cvars_iz = $(cvars) $(CF_LOC)
conlibs_iz = $(conlibs) $(ZLIBLIB) $(BZIPLIBEXE)
guilibs_iz = $(guilibs) $(ZLIBLIB) $(BZIPLIBEXE)

!IFDEF DLLSTANDALONE
# The standalone variant of unzip32.dll is linked against the
# multithread-safe static C runtime library.
cvars_izdll = $(cvarsmt) $(CF_LOC)
conlibs_izdll = $(conlibsmt) $(ZLIBLIB) $(BZIPLIBDLL)
!ELSE
# The (normal) variant of unzip32.dll is linked against the import library
# of the multithread-safe C runtime dll.
# -> smaller dll size; efficient use of shared resources,
#    but requires the C RTL DLL msvcrt.dll to be installed at runtime
cvars_izdll = $(cvarsdll) $(CF_LOC)
conlibs_izdll = $(conlibsdll) $(ZLIBLIBDLL) $(BZIPLIBDLL)
!ENDIF
!ENDIF # ?(LINK_CRTDLL == 1)

CFLAGS_UN = $(cdebug) $(cflags) $(INC_BZ2LIB) $(cvars_iz)
CFLAGS_SX = $(cdbgsz) $(cflags) $(INC_BZ2LIB) $(cvars_iz) -DSFX
CFLAGS_FU = $(cdebug) $(cflags) $(cvars_iz) -DFUNZIP
CFLAGS_DL = $(cdebug) $(cflags) $(INC_BZ2LIB) $(cvars_izdll) -DWINDLL -DDLL
CFLAGS_DX = $(cdebug) $(cflags) $(INC_BZ2LIB) $(cvars_izdll)
CFLAGS_SL = $(cdbgsz) $(cflags) $(INC_BZ2LIB) $(cvars_iz) -DWINDLL -DDLL -DUNZIPLIB -DSFX
CFLAGS_GX = $(cdbgsz) $(cflags) $(cvars_iz) -DSFX
CFLAGS_LB = $(cdebug) $(cflags) $(INC_BZ2LIB) $(cvars_iz) -DWINDLL -DDLL -DUNZIPLIB

# You may define VC6LINK if your linker recognizes "-opt:nowin98" alignment
# qualifier. (VC6 or newer). This option reduces the executable size of the
# SFX stubs (Instead, you might apply the free executable compressor "upx"
# which is far more efficient in reducing the stub's file size.)
!IFDEF VC6LINK
LINKOPT_SIZE = -opt:nowin98
!ENDIF

# How to compile sources
.c.obj:
	$(cc) $(CFLAGS_UN) -Fo$@ $<

# How to link
.obj.exe:
	$(link) $(ldebug) $(conlflags) $(conlibs_iz) $** -out:$@


# Default target is all command-line executables
unzips: unzip.exe funzip.exe unzipsfx.exe
guisfx: SFXWiz32.exe
dll: unzip32.dll
dllsample: uzexampl.exe
lib: $(OBDIR) $(OB)unzip32.lib
all: unzips guisfx dll dllsample lib


unzip.exe:	$(OBJU) $(BZIPLIBEXE) winapp.res
	$(link) $(ldebug) $(conlflags) $(conlibs_iz) $** -out:$@
unzipsfx.exe:	$(OBJX) $(BZIPLIBEXE)
	$(link) $(ldebug) $(conlflags) $(conlibs_iz) $** $(LINKOPT_SIZE) -out:$@
funzip.exe:	$(OBJF)
unzip32.dll:	$(OBJD) $(BZIPLIBDLL) windll.res $(WINDLL_DEF)
	$(link) $(ldebug) $(dlllflags) $(conlibs_izdll) -def:$(WINDLL_DEF) \
		$(OBJD) $(BZIPLIBDLL) windll.res -out:$@
unzsfx32.lib:	$(OBLX)
	$(link) -lib $** -out:$@
SFXWiz32.exe:	$(OBGX) sfxwiz.res unzsfx32.lib $(BZIPLIBEXE)
	$(link) $(ldebug) $(guilflags) $(guilibs_iz) \
		$(OBGX) sfxwiz.res unzsfx32.lib $(LINKOPT_SIZE) -out:$@
uzexampl.exe:	uzexampl.obj
	$(link) $(ldebug) $(conlflags) $(conlibs) version.lib $** -out:$@
$(OB)unzip32.lib:	$(OBJB)
	$(link) -lib $** -out:$@

$(IZ_BZIP2)/$(OBEXE)/bz2.lib:
	$(MAKE) -f $(IZ_BZIP2)/makbz2iz.msc \
	CFLSYS="$(cdebug) $(cflags) $(cvars_iz)" \
	BZROOTDIR=$(IZ_BZIP2) BZOBDIR=$(IZ_BZIP2)\$(OBEXE)

$(IZ_BZIP2)/$(OBDLL)/bz2.lib:
	$(MAKE) -f $(IZ_BZIP2)/makbz2iz.msc \
	CFLSYS="$(cdebug) $(cflags) $(cvars_izdll)" \
	BZROOTDIR=$(IZ_BZIP2) BZOBDIR=$(IZ_BZIP2)\$(OBDLL)

crc32.obj:	crc32.c $(UNZIP_H) zip.h crc32.h
crypt.obj:	crypt.c $(UNZIP_H) zip.h crypt.h crc32.h ttyio.h
envargs.obj:	envargs.c $(UNZIP_H)
explode.obj:	explode.c $(UNZIP_H)
extract.obj:	extract.c $(UNZIP_H) crc32.h crypt.h
fileio.obj:	fileio.c $(UNZIP_H) crc32.h crypt.h ttyio.h ebcdic.h
funzip.obj:	funzip.c $(UNZIP_H) crc32.h crypt.h ttyio.h
globals.obj:	globals.c $(UNZIP_H)
inflate.obj:	inflate.c inflate.h $(UNZIP_H)
list.obj:	list.c $(UNZIP_H)
match.obj:	match.c $(UNZIP_H)
process.obj:	process.c $(UNZIP_H) crc32.h
timezone.obj:	timezone.c $(UNZIP_H) zip.h timezone.h
ttyio.obj:	ttyio.c $(UNZIP_H) zip.h crypt.h ttyio.h
ubz2err.obj:	ubz2err.c $(UNZIP_H)
unreduce.obj:	unreduce.c $(UNZIP_H)
unshrink.obj:	unshrink.c $(UNZIP_H)
unzip.obj:	unzip.c $(UNZIP_H) crypt.h unzvers.h consts.h
zipinfo.obj:	zipinfo.c $(UNZIP_H)

win32.obj:	win32/win32.c $(UNZIP_H) win32/nt.h
	$(cc) $(CFLAGS_UN) -I. win32/win32.c -Fo$@

win32i64.obj:	win32/win32i64.c $(UNZIP_H)
	$(cc) $(CFLAGS_UN) -I. win32/win32i64.c -Fo$@

nt.obj:		win32/nt.c $(UNZIP_H) win32/nt.h
	$(cc) $(CFLAGS_UN) -I. win32/nt.c -Fo$@

!ifdef USEMASM
crc_i386.obj:	win32/crc_i386.asm
	masm -ml win32/crc_i386.asm,$@;
!else
!ifdef USEML
crc_i386.obj:	win32/crc_i386.asm
	ml -c -Cp -coff -Fo$@ win32/crc_i386.asm
!else
crc_i386.obj:	win32/crc_i386.c
	$(cc) $(CFLAGS_UN) -I. win32/crc_i386.c -Fo$@
!endif
!endif

# UnZipSFX compilation section
crc32_.obj:	crc32.c $(UNZIP_H) zip.h crc32.h
	$(cc) $(CFLAGS_SX) crc32.c -Fo$@
crypt_.obj:	crypt.c $(UNZIP_H) zip.h crypt.h crc32.h ttyio.h
	$(cc) $(CFLAGS_SX) crypt.c -Fo$@
extract_.obj:	extract.c $(UNZIP_H) crc32.h crypt.h
	$(cc) $(CFLAGS_SX) extract.c -Fo$@
fileio_.obj:	fileio.c $(UNZIP_H) crc32.h crypt.h ttyio.h ebcdic.h
	$(cc) $(CFLAGS_SX) fileio.c -Fo$@
globals_.obj:	globals.c $(UNZIP_H)
	$(cc) $(CFLAGS_SX) globals.c -Fo$@
inflate_.obj:	inflate.c inflate.h $(UNZIP_H) crypt.h
	$(cc) $(CFLAGS_SX) inflate.c -Fo$@
match_.obj:	match.c $(UNZIP_H)
	$(cc) $(CFLAGS_SX) match.c -Fo$@
process_.obj:	process.c $(UNZIP_H) crc32.h
	$(cc) $(CFLAGS_SX) process.c -Fo$@
timezon_.obj:	timezone.c $(UNZIP_H) zip.h timezone.h
	$(cc) $(CFLAGS_SX) timezone.c -Fo$@
ttyio_.obj:	ttyio.c $(UNZIP_H) zip.h crypt.h ttyio.h
	$(cc) $(CFLAGS_SX) ttyio.c -Fo$@
ubz2err_.obj:	ubz2err.c $(UNZIP_H)
	$(cc) $(CFLAGS_SX) ubz2err.c -Fo$@
unzipsfx.obj:	unzip.c $(UNZIP_H) crypt.h unzvers.h consts.h
	$(cc) $(CFLAGS_SX) unzip.c -Fo$@

win32_.obj:	win32/win32.c $(UNZIP_H) win32/nt.h
	$(cc) $(CFLAGS_SX) -I. win32/win32.c -Fo$@

win32i64_.obj:	win32/win32i64.c $(UNZIP_H)
	$(cc) $(CFLAGS_SX) -I. win32/win32i64.c -Fo$@

nt_.obj:	win32/nt.c $(UNZIP_H) win32/nt.h
	$(cc) $(CFLAGS_SX) -I. win32/nt.c -Fo$@

# fUnZip compilation section
crc32f.obj:	crc32.c $(UNZIP_H) zip.h crc32.h
	$(cc) $(CFLAGS_FU) crc32.c -Fo$@
cryptf.obj:	crypt.c $(UNZIP_H) zip.h crypt.h crc32.h ttyio.h
	$(cc) $(CFLAGS_FU) crypt.c -Fo$@
globalsf.obj:	globals.c $(UNZIP_H)
	$(cc) $(CFLAGS_FU) globals.c -Fo$@
inflatef.obj:	inflate.c inflate.h $(UNZIP_H) crypt.h
	$(cc) $(CFLAGS_FU) inflate.c -Fo$@
ttyiof.obj:	ttyio.c $(UNZIP_H) zip.h crypt.h ttyio.h
	$(cc) $(CFLAGS_FU) ttyio.c -Fo$@

win32f.obj:	win32/win32.c $(UNZIP_H) win32/nt.h
	$(cc) $(CFLAGS_FU) -I. win32/win32.c -Fo$@

win32i64f.obj:	win32/win32i64.c $(UNZIP_H)
	$(cc) $(CFLAGS_FU) -I. win32/win32i64.c -Fo$@

# DLL compilation section
api.obj:	api.c $(UNZIP_H) $(WINDLL_H) unzvers.h
	$(cc) $(CFLAGS_DL) api.c -Fo$@
crc32l.obj:	crc32.c $(UNZIP_H) zip.h crc32.h
	$(cc) $(CFLAGS_DL) crc32.c -Fo$@
cryptl.obj:	crypt.c $(UNZIP_H) zip.h crypt.h crc32.h ttyio.h
	$(cc) $(CFLAGS_DL) crypt.c -Fo$@
explodel.obj:	explode.c $(UNZIP_H)
	$(cc) $(CFLAGS_DL) explode.c -Fo$@
extractl.obj:	extract.c $(UNZIP_H) $(WINDLL_H) crc32.h crypt.h
	$(cc) $(CFLAGS_DL) extract.c -Fo$@
fileiol.obj:	fileio.c $(UNZIP_H) $(WINDLL_H) crc32.h crypt.h ttyio.h ebcdic.h
	$(cc) $(CFLAGS_DL) fileio.c -Fo$@
globalsl.obj:	globals.c $(UNZIP_H)
	$(cc) $(CFLAGS_DL) globals.c -Fo$@
inflatel.obj:	inflate.c inflate.h $(UNZIP_H)
	$(cc) $(CFLAGS_DL) inflate.c -Fo$@
listl.obj:	list.c $(UNZIP_H) $(WINDLL_H)
	$(cc) $(CFLAGS_DL) list.c -Fo$@
matchl.obj:	match.c $(UNZIP_H)
	$(cc) $(CFLAGS_DL) match.c -Fo$@
processl.obj:	process.c $(UNZIP_H) $(WINDLL_H) crc32.h
	$(cc) $(CFLAGS_DL) process.c -Fo$@
timezonl.obj:	timezone.c $(UNZIP_H) zip.h timezone.h
	$(cc) $(CFLAGS_DL) timezone.c -Fo$@
ubz2errl.obj:	ubz2err.c $(UNZIP_H)
	$(cc) $(CFLAGS_DL) ubz2err.c -Fo$@
unreducl.obj:	unreduce.c $(UNZIP_H)
	$(cc) $(CFLAGS_DL) unreduce.c -Fo$@
unshrnkl.obj:	unshrink.c $(UNZIP_H)
	$(cc) $(CFLAGS_DL) unshrink.c -Fo$@
zipinfol.obj:	zipinfo.c $(UNZIP_H)
	$(cc) $(CFLAGS_DL) zipinfo.c -Fo$@

win32l.obj:	win32/win32.c $(UNZIP_H) win32/nt.h
	$(cc) $(CFLAGS_DL) -I. win32/win32.c -Fo$@

win32i64l.obj:	win32/win32i64.c $(UNZIP_H)
	$(cc) $(CFLAGS_DL) -I. win32/win32i64.c -Fo$@

ntl.obj:	win32/nt.c $(UNZIP_H) win32/nt.h
	$(cc) $(CFLAGS_DL) -I. win32/nt.c -Fo$@

windll.obj:  windll/windll.c $(UNZIP_H) $(WINDLL_H) crypt.h unzvers.h consts.h
	$(cc) $(CFLAGS_DL) -I. windll/windll.c -Fo$@

!ifdef USEMASM
crci386l.obj:	win32/crc_i386.asm
	masm -ml win32/crc_i386.asm,$@;
!else
!ifdef USEML
crci386l.obj:	win32/crc_i386.asm
	ml -c -Cp -coff -Fo$@ win32/crc_i386.asm
!else
crci386l.obj:	win32/crc_i386.c
	$(cc) $(CFLAGS_DL) -I. win32/crc_i386.c -Fo$@
!endif
!endif

winapp.res:	win32/winapp.rc unzvers.h
	$(rc) /l 0x409 /fo$@ /i win32 /d WIN32 win32/winapp.rc

windll.res:	windll/windll.rc unzvers.h
	$(rc) /l 0x409 /fo$@ /i windll /d WIN32 windll/windll.rc

uzexampl.obj:	windll/uzexampl.c windll/uzexampl.h
	$(cc) $(CFLAGS_DX) -I. windll/uzexampl.c -Fo$@

# SFX Lib compilation section
apig.obj:	api.c $(UNZIP_H) $(WINDLL_H) unzvers.h
	$(cc) $(CFLAGS_SL) api.c -Fo$@
crc32g.obj:	crc32.c $(UNZIP_H) zip.h crc32.h
	$(cc) $(CFLAGS_SL) crc32.c -Fo$@
cryptg.obj:	crypt.c $(UNZIP_H) zip.h crypt.h crc32.h ttyio.h
	$(cc) $(CFLAGS_SL) crypt.c -Fo$@
extractg.obj:	extract.c $(UNZIP_H) $(WINDLL_H) crc32.h crypt.h
	$(cc) $(CFLAGS_SL) extract.c -Fo$@
fileiog.obj:	fileio.c $(UNZIP_H) $(WINDLL_H) crc32.h crypt.h ttyio.h ebcdic.h
	$(cc) $(CFLAGS_SL) fileio.c -Fo$@
globalsg.obj:	globals.c $(UNZIP_H)
	$(cc) $(CFLAGS_SL) globals.c -Fo$@
inflateg.obj:	inflate.c inflate.h $(UNZIP_H)
	$(cc) $(CFLAGS_SL) inflate.c -Fo$@
matchg.obj:	match.c $(UNZIP_H)
	$(cc) $(CFLAGS_SL) match.c -Fo$@
processg.obj:	process.c $(UNZIP_H) $(WINDLL_H) crc32.h
	$(cc) $(CFLAGS_SL) process.c -Fo$@
timezong.obj:	timezone.c $(UNZIP_H) zip.h timezone.h
	$(cc) $(CFLAGS_SL) timezone.c -Fo$@
ubz2errg.obj:	ubz2err.c $(UNZIP_H)
	$(cc) $(CFLAGS_SL) ubz2err.c -Fo$@

win32g.obj:	win32/win32.c $(UNZIP_H) win32/nt.h
	$(cc) $(CFLAGS_SL) -I. win32/win32.c -Fo$@

win32i64g.obj:	win32/win32i64.c $(UNZIP_H)
	$(cc) $(CFLAGS_SL) -I. win32/win32i64.c -Fo$@

ntg.obj:	win32/nt.c $(UNZIP_H) win32/nt.h
	$(cc) $(CFLAGS_SL) -I. win32/nt.c -Fo$@

windllg.obj:  windll/windll.c $(UNZIP_H) $(WINDLL_H) crypt.h unzvers.h consts.h
	$(cc) $(CFLAGS_SL) -I. windll/windll.c -Fo$@

!ifdef USEMASM
crci386g.obj:	win32/crc_i386.asm
	masm -ml win32/crc_i386.asm,$@;
!else
!ifdef USEML
crci386g.obj:	win32/crc_i386.asm
	ml -c -Cp -coff -Fo$@ win32/crc_i386.asm
!else
crci386g.obj:	win32/crc_i386.c
	$(cc) $(CFLAGS_SL) -I. win32/crc_i386.c -Fo$@
!endif
!endif

sfxwiz.obj:	windll/guisfx/sfxwiz.c windll/guisfx/dialog.h $(WINDLL_IMP_H)
	$(cc) $(CFLAGS_GX) windll/guisfx/sfxwiz.c -Fo$@

sfxwiz.res:	windll/guisfx/sfxwiz.rc
	$(rc) /l 0x409 /fo$@ /i windll/guisfx /d WIN32 windll/guisfx/sfxwiz.rc

# static LIB compilation section
$(OB)apib.obj:	api.c $(UNZIP_H) $(WINDLL_H) unzvers.h
	$(cc) $(CFLAGS_LB) api.c -Fo$@
$(OB)crc32b.obj:	crc32.c $(UNZIP_H) zip.h crc32.h
	$(cc) $(CFLAGS_LB) crc32.c -Fo$@
$(OB)cryptb.obj:	crypt.c $(UNZIP_H) zip.h crypt.h crc32.h ttyio.h
	$(cc) $(CFLAGS_LB) crypt.c -Fo$@
$(OB)explodeb.obj:	explode.c $(UNZIP_H)
	$(cc) $(CFLAGS_LB) explode.c -Fo$@
$(OB)extractb.obj:	extract.c $(UNZIP_H) $(WINDLL_H) crc32.h crypt.h
	$(cc) $(CFLAGS_LB) extract.c -Fo$@
$(OB)fileiob.obj:	fileio.c $(UNZIP_H) $(WINDLL_H) crc32.h crypt.h ttyio.h ebcdic.h
	$(cc) $(CFLAGS_LB) fileio.c -Fo$@
$(OB)globalsb.obj:	globals.c $(UNZIP_H)
	$(cc) $(CFLAGS_LB) globals.c -Fo$@
$(OB)inflateb.obj:	inflate.c inflate.h $(UNZIP_H)
	$(cc) $(CFLAGS_LB) inflate.c -Fo$@
$(OB)listb.obj:	list.c $(UNZIP_H) $(WINDLL_H)
	$(cc) $(CFLAGS_LB) list.c -Fo$@
$(OB)matchb.obj:	match.c $(UNZIP_H)
	$(cc) $(CFLAGS_LB) match.c -Fo$@
$(OB)processb.obj:	process.c $(UNZIP_H) $(WINDLL_H) crc32.h
	$(cc) $(CFLAGS_LB) process.c -Fo$@
$(OB)timezonb.obj:	timezone.c $(UNZIP_H) zip.h timezone.h
	$(cc) $(CFLAGS_LB) timezone.c -Fo$@
$(OB)ubz2errb.obj:	ubz2err.c $(UNZIP_H)
	$(cc) $(CFLAGS_LB) ubz2err.c -Fo$@
$(OB)unreducb.obj:	unreduce.c $(UNZIP_H)
	$(cc) $(CFLAGS_LB) unreduce.c -Fo$@
$(OB)unshrnkb.obj:	unshrink.c $(UNZIP_H)
	$(cc) $(CFLAGS_LB) unshrink.c -Fo$@
$(OB)zipinfob.obj:	zipinfo.c $(UNZIP_H)
	$(cc) $(CFLAGS_LB) zipinfo.c -Fo$@

$(OB)win32b.obj:	win32/win32.c $(UNZIP_H) win32/nt.h
	$(cc) $(CFLAGS_LB) -I. win32/win32.c -Fo$@

$(OB)win32i64b.obj:	win32/win32i64.c $(UNZIP_H)
	$(cc) $(CFLAGS_LB) -I. win32/win32i64.c -Fo$@

$(OB)ntb.obj:	win32/nt.c $(UNZIP_H) win32/nt.h
	$(cc) $(CFLAGS_LB) -I. win32/nt.c -Fo$@

$(OB)windllb.obj:  windll/windll.c $(UNZIP_H) $(WINDLL_H) crypt.h unzvers.h consts.h
	$(cc) $(CFLAGS_LB) -I. windll/windll.c -Fo$@

!ifdef USEMASM
$(OB)crci386b.obj:	win32/crc_i386.asm
	masm -ml win32/crc_i386.asm,$@;
!else
!ifdef USEML
$(OB)crci386b.obj:	win32/crc_i386.asm
	ml -c -Cp -coff -Fo$@ win32/crc_i386.asm
!else
$(OB)crci386b.obj:	win32/crc_i386.c
	$(cc) $(CFLAGS_LB) -I. win32/crc_i386.c -Fo$@
!endif
!endif

$(OBDIR):
	-mkdir $@

clean:	clean_bz2exe_lib clean_bz2dll_lib
	-del *.obj
	-del winapp.res
	-del unzip.exe
	-del unzip.pdb
	-del funzip.exe
	-del funzip.pdb
	-del unzipsfx.exe
	-del unzipsfx.pdb
	-del vc60.pdb
	-del sfxwiz.res
	-del SFXWiz32.exe
	-del windll.res
	-del unzip32.dll
	-del unzip32.lib
	-del unzip32.exp
	-del unzip32.dll.manifest
	-del uzexampl.exe
	-del unzsfx32.lib
	-del $(OBDIR)\*.obj
	-del $(OBDIR)\unzip32.lib
	-rmdir $(OBDIR)

clean_bz2exe_lib:
	-$(MAKE) -f $(IZ_BZIP2)/makbz2iz.msc \
	BZROOTDIR=$(IZ_BZIP2) BZOBDIR=$(IZ_BZIP2)\$(OBEXE) clean

clean_bz2dll_lib:
	-$(MAKE) -f $(IZ_BZIP2)/makbz2iz.msc \
	BZROOTDIR=$(IZ_BZIP2) BZOBDIR=$(IZ_BZIP2)\$(OBDLL) clean
