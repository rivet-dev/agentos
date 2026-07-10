# WMAKE makefile for 16 bit MSDOS or 32 bit DOS extender (PMODE/W or DOS/4GW)
# using Watcom C/C++ v11.0+, by Paul Kienitz, last revised 16 Feb 2008.
# Makes UnZip.exe, fUnZip.exe, and UnZipSFX.exe.
#
# Invoke from UnZip source dir with "WMAKE -F MSDOS\MAKEFILE.WAT [targets]".
# To make the PMODE/W version use "WMAKE PM=1 ..."
# To make the DOS/4GW version use "WMAKE GW=1 ..." (overrides PM=1)
#   Note: specifying PM or GW without NOASM requires that the win32 source
#   directory be present, so it can access the 32 bit assembly source.
#   PMODE/W is recommended over DOS/4GW for best performance.
# To build with debug info use "WMAKE DEBUG=1 ..."
# To build with no assembly modules use "WMAKE NOASM=1 ..."
# To omit unshrinking support use "WMAKE NO_LZW=1 ..."
# To support unreducing, get the real unreduce.c and go "WMAKE OFFEND_RMS=1 ..."
# To include support for bzip2 decompression (only for 32-bit), get the bzip2
#  source distribution into the bzip2/ subfolder and start compilation
#  with "WMAKE PM=1 USEBZ2=1 ..."
#
# Other options to be fed to the compiler can be specified in an environment
# variable called LOCAL_UNZIP.

variation = $(%LOCAL_UNZIP)

# Stifle annoying "Delete this file?" questions when errors occur:
.ERASE

.EXTENSIONS:
.EXTENSIONS: .exe .obj .obx .c .h .asm

# We maintain multiple sets of object files in different directories so that
# we can compile msdos, dos/4gw, and win32 versions of UnZip without their
# object files interacting.  The following var must be a directory name
# ending with a backslash.  All object file names must include this macro
# at the beginning, for example "$(O)foo.obj".

!ifdef GW
PM = 1      # both protected mode formats use the same object files
!endif

!ifdef DEBUG
!  ifdef PM
OBDIR = od32d
!  else
OBDIR = od16d
!  endif
!else
!  ifdef PM
OBDIR = ob32d
!  else
OBDIR = ob16d
!  endif
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
!ifdef PM
cvars = $+$(cvars)$- -DUSE_BZIP2 -I$(IZ_BZIP2)
bz2errob = $(O)ubz2err.obj
bz2errox = $(O)ubz2err.obj
BZIPLIB = $(IZ_BZIP2)/$(OBDIR)/bz2.lib
BZ2LNKLIB = lib {$(BZIPLIB)}
!else
bz2errob =
bz2errox =
BZIPLIB =
BZ2LNKLIB =
!endif
!else
bz2errob =
bz2errox =
BZIPLIB =
BZ2LNKLIB =
!endif

# The assembly hot-spot code in crc_i[3]86.asm is optional.  This section
# controls its usage.

!ifdef NOASM
crcaob =
crcaof =
crcaox =
!else   # !NOASM
cvars = $+$(cvars)$- -DASM_CRC
!  ifdef PM
crcaob = $(O)crc_i386.obj
crcaof = $(O)crc_i38f.obj
crcaox = $(O)crc_i386.obx
crc_s = win32\crc_i386.asm   # requires that the win32 directory be present
!  else
crcaob = $(O)crc_i86.obj
crcaof = $(O)crc_i8f.obj
crcaox = $(O)crc_i86.obx
crc_s = msdos\crc_i86.asm
!  endif
!endif

# Our object files: OBJ[A-C] is for UnZip, OBJX for UnZipSFX, OBJF for fUnZip:

OBJA1 = $(O)unzip.obj $(O)crc32.obj $(crcaob) $(O)crypt.obj $(O)envargs.obj
OBJA  = $(OBJA1) $(O)explode.obj $(O)extract.obj $(O)fileio.obj $(O)globals.obj
OBJB2 = $(O)inflate.obj $(O)list.obj $(O)match.obj $(O)process.obj
OBJB  = $(OBJB2) $(O)ttyio.obj $(bz2errob) $(O)unreduce.obj
OBJC  = $(O)unshrink.obj $(O)zipinfo.obj $(O)msdos.obj

OBJX2 = $(O)unzip.obx $(O)crc32.obx $(crcaox) $(O)crypt.obx $(O)extract.obx
OBJX1 = $(OBJX2) $(O)fileio.obx $(O)globals.obx $(O)inflate.obx $(O)match.obx
OBJX  = $(OBJX1) $(O)process.obx $(bz2errox) $(O)ttyio.obx $(O)msdos.obx

OBJF1  = $(O)funzip.obj $(O)crc32f.obj $(crcaof) $(O)cryptf.obj $(O)globalsf.obj
OBJF  = $(OBJF1) $(O)inflatef.obj $(O)ttyiof.obj $(O)msdosf.obj

# Common header files included by all C sources:

UNZIP_H = unzip.h unzpriv.h globals.h msdos\doscfg.h

# Now we have to pick out the proper compiler and options for it.  This gets
# pretty complicated with the PM, GW, DEBUG, and NOASM options...

link   = wlink
asm    = wasm

!ifdef PM
cc     = wcc386
# Use Pentium Pro timings, flat memory, static strings in code, max strictness:
cflags = -bt=DOS -mf -6r -zt -zq -wx
aflags = -bt=DOS -mf -3 -zq
cflagf = $(cflags)
aflagf = $(aflags)
cflagx = $(cflags)
aflagx = $(aflags)

!  ifdef GW
lflags = sys DOS4G
!  else
# THIS REQUIRES THAT PMODEW.EXE BE FINDABLE IN THE COMMAND PATH.
# It does NOT require you to add a pmodew entry to wlink.lnk or wlsystem.lnk.
defaultlibs = libpath %WATCOM%\lib386 libpath %WATCOM%\lib386\dos
lflags = format os2 le op osname='PMODE/W' op stub=pmodew.exe $(defaultlibs)
!  endif

!else   # plain 16-bit DOS:

cc     = wcc
# Use plain 8086 code, large memory, static strings in code, max strictness:
cflags = -bt=DOS -ml -0 -zt -zq -wx
aflags = -bt=DOS -ml -0 -zq
# for fUnZip, Deflate64 support requires the compact memory model:
cflagf = -bt=DOS -mc -0 -zt -zq -wx
aflagf = -bt=DOS -mc -0 -zq
# for UnZipSFX (without Deflate64 support), use the small memory model:
cflagx = -bt=DOS -ms -0 -zt -zq -wx
aflagx = -bt=DOS -ms -0 -zq
lflags = sys DOS
!endif  # !PM

cvars  = $+$(cvars)$- -DMSDOS $(variation)
avars  = $+$(avars)$- $(variation)


# Specify optimizations, or a nonoptimized debugging version:

!ifdef DEBUG
cdebug = -od -d2
cdebux = -od -d2
ldebug = d w all op symf
!else
!  ifdef PM
cdebug = -s -obhikl+rt -oe=100 -zp8
# -oa helps slightly but might be dangerous.
!  else
cdebug = -s -oehiklrt
!  endif
cdebux = -s -obhiklrs
ldebug = op el
!endif

# How to compile sources:
.c.obx:
	$(cc) $(cdebux) $(cflagx) $(cvars) -DSFX $[@ -fo=$@

.c.obj:
	$(cc) $(cdebug) $(cflags) $(cvars) $[@ -fo=$@

# Here we go!  By default, make all targets, except no UnZipSFX for PMODE:
!ifdef PM
all: UnZip.exe fUnZip.exe
!else
all: UnZip.exe fUnZip.exe UnZipSFX.exe
!endif

# Convenient shorthand options for single targets:
u:   UnZip.exe     .SYMBOLIC
f:   fUnZip.exe    .SYMBOLIC
x:   UnZipSFX.exe  .SYMBOLIC

UnZip.exe:	$(OBDIR) $(OBJA) $(OBJB) $(OBJC) $(BZIPLIB)
	set WLK_VA=file {$(OBJA)}
	set WLK_VB=file {$(OBJB)}
        set WLK_VC=file {$(OBJC)} $(BZ2LNKLIB)
	$(link) $(lflags) $(ldebug) name $@ @WLK_VA @WLK_VB @WLK_VC
	set WLK_VA=
	set WLK_VB=
        set WLK_VC=
# We use WLK_VA/WLK_VB/WLK_VC to keep the size of each command below 256 chars.

UnZipSFX.exe:	$(OBDIR) $(OBJX) $(BZIPLIB)
	set WLK_VX=file {$(OBJX)}
	$(link) $(lflags) $(ldebug) name $@ @WLK_VX
	set WLK_VX=

fUnZip.exe:	$(OBDIR) $(OBJF)
	set WLK_VF=file {$(OBJF)}
	$(link) $(lflags) $(ldebug) name $@ @WLK_VF
	set WLK_VF=


# Source dependencies:

#       for UnZip ...

$(O)crc32.obj:    crc32.c $(UNZIP_H) zip.h crc32.h
$(O)crypt.obj:    crypt.c $(UNZIP_H) zip.h crypt.h crc32.h ttyio.h
$(O)envargs.obj:  envargs.c $(UNZIP_H)
$(O)explode.obj:  explode.c $(UNZIP_H)
$(O)extract.obj:  extract.c $(UNZIP_H) crc32.h crypt.h
$(O)fileio.obj:   fileio.c $(UNZIP_H) crc32.h crypt.h ttyio.h ebcdic.h
$(O)globals.obj:  globals.c $(UNZIP_H)
$(O)inflate.obj:  inflate.c inflate.h $(UNZIP_H)
$(O)list.obj:     list.c $(UNZIP_H)
$(O)match.obj:    match.c $(UNZIP_H)
$(O)process.obj:  process.c $(UNZIP_H) crc32.h
$(O)ttyio.obj:    ttyio.c $(UNZIP_H) zip.h crypt.h ttyio.h
$(O)ubz2err.obj:  ubz2err.c $(UNZIP_H)
$(O)unreduce.obj: unreduce.c $(UNZIP_H)
$(O)unshrink.obj: unshrink.c $(UNZIP_H)
$(O)unzip.obj:    unzip.c $(UNZIP_H) crypt.h unzvers.h consts.h
$(O)zipinfo.obj:  zipinfo.c $(UNZIP_H)

#       for UnZipSFX ...

$(O)crc32.obx:    crc32.c $(UNZIP_H) zip.h crc32.h
$(O)crypt.obx:    crypt.c $(UNZIP_H) zip.h crypt.h crc32.h ttyio.h
$(O)extract.obx:  extract.c $(UNZIP_H) crc32.h crypt.h
$(O)fileio.obx:   fileio.c $(UNZIP_H) crc32.h crypt.h ttyio.h ebcdic.h
$(O)globals.obx:  globals.c $(UNZIP_H)
$(O)inflate.obx:  inflate.c inflate.h $(UNZIP_H)
$(O)match.obx:    match.c $(UNZIP_H)
$(O)process.obx:  process.c $(UNZIP_H) crc32.h
$(O)ttyio.obx:    ttyio.c $(UNZIP_H) zip.h crypt.h ttyio.h
$(O)ubz2err.obx:  ubz2err.c $(UNZIP_H)
$(O)unzip.obx:    unzip.c $(UNZIP_H) crypt.h unzvers.h consts.h

# Special case object files:

$(O)msdos.obj:    msdos\msdos.c $(UNZIP_H)
	$(cc) $(cdebug) $(cflags) $(cvars) msdos\msdos.c -fo=$@

$(O)msdos.obx:    msdos\msdos.c $(UNZIP_H)
	$(cc) $(cdebux) $(cflagx) $(cvars) -DSFX msdos\msdos.c -fo=$@

!ifndef NOASM
$(crcaob):        $(crc_s)
	$(asm) $(aflags) $(avars) $(crc_s) -fo=$@

$(crcaof):        $(crc_s)
	$(asm) $(aflagf) $(avars) $(crc_s) -fo=$@

$(crcaox):        $(crc_s)
	$(asm) $(aflagx) $(avars) $(crc_s) -fo=$@
!endif

# Variant object files for fUnZip, using $(cflagf):

$(O)funzip.obj:   funzip.c $(UNZIP_H) crc32.h crypt.h ttyio.h
	$(cc) $(cdebux) $(cflagf) $(cvars) funzip.c -fo=$@

$(O)crc32f.obj:   crc32.c $(UNZIP_H) zip.h crc32.h
	$(cc) $(cdebux) $(cflagf) $(cvars) -DFUNZIP crc32.c -fo=$@

$(O)cryptf.obj:   crypt.c $(UNZIP_H) zip.h crypt.h crc32.h ttyio.h
	$(cc) $(cdebux) $(cflagf) $(cvars) -DFUNZIP crypt.c -fo=$@

$(O)globalsf.obj: globals.c $(UNZIP_H)
	$(cc) $(cdebux) $(cflagf) $(cvars) -DFUNZIP globals.c -fo=$@

$(O)inflatef.obj: inflate.c inflate.h $(UNZIP_H) crypt.h
	$(cc) $(cdebux) $(cflagf) $(cvars) -DFUNZIP inflate.c -fo=$@

$(O)ttyiof.obj:   ttyio.c $(UNZIP_H) zip.h crypt.h ttyio.h
	$(cc) $(cdebux) $(cflagf) $(cvars) -DFUNZIP ttyio.c -fo=$@

$(O)msdosf.obj:    msdos\msdos.c $(UNZIP_H)
	$(cc) $(cdebux) $(cflagf) $(cvars) -DFUNZIP msdos\msdos.c -fo=$@

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
	$(MAKE) -h -f $(IZ_BZIP2)/makbz2iz.wat &
	BZROOTDIR=$(IZ_BZIP2) BZOBDIR=$(IZ_BZIP2)\$(OBDIR) clean

clean:     clean_bz2_lib  .SYMBOLIC
	del $(O)*.ob?

cleaner:   clean  .SYMBOLIC
	del UnZip.exe
	del fUnZip.exe
	del UnZipSFX.exe
