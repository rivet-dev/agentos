#==============================================================================
# Makefile for UnZip, UnZipSFX and fUnZip:  Unix and MS-DOS ("real" makes only)
# Version:  5.53                                               25 December 2006
#==============================================================================


# INSTRUCTIONS (such as they are):
#
# "make vax"	-- makes UnZip on a generic Unix VAX in the current directory
# "make list"	-- lists all supported systems (targets)
# "make help"	-- provides pointers on what targets to try if problems occur
# "make wombat" -- chokes and dies if you haven't added the specifics for your
#		    Wombat 68000 (or whatever) to the systems list
#
# CF are flags for the C compiler.  LF are flags for the loader.  LF2 are more
# flags for the loader, if they need to be at the end of the line instead of at
# the beginning (for example, some libraries).  FL and FL2 are the corre-
# sponding flags for fUnZip.  LOCAL_UNZIP is an environment variable that can
# be used to add default C flags to your compile without editing the Makefile
# (e.g., -DDEBUG_STRUC, or -FPi87 on PCs using Microsoft C).
#
# Some versions of make do not define the macro "$(MAKE)"; this is rare, but
# if things don't work, try using "make" instead of "$(MAKE)" in your system's
# makerule.  Or try adding the following line to your .login file:
#	setenv MAKE "make"
# (That never works--makes which are too stupid to define MAKE are also too
# stupid to look in the environment--but try it anyway for kicks. :-) )
#
# Memcpy and memset are provided for those systems that don't have them; they
# are in file_io.c and will be used if -DZMEM is included in CF.  These days
# almost all systems have them.
#
# Be sure to test your new UnZip (and UnZipSFX and fUnZip); successful compila-
# tion does not always imply a working program.


#####################
# MACRO DEFINITIONS #
#####################

# Defaults most systems use (use LOCAL_UNZIP in environment to add flags,
# such as -DDOSWILD).

# UnZip flags
CC = cl
LD = cl
LOC = $(LOCAL_UNZIP)
CF = -c -Zi -W2 -DCRYPT -DOLD_THEOS_EXTRA $(LOCAL_UNZIP)
LF = -Zi -o
LF2 = -m

# defaults for crc32 stuff and system dependent headers
CRCA_O =
OSDEP_H =

# general-purpose stuff
CP = copyfile
LN = copyfile
RM = erase

.c.o:
	$(CC) $(CF) $*.c

# object files
OBJS1 = unzip.o crc32.o $(CRCA_O) crypt.o envargs.o explode.o
OBJS2 = extract.o fileio.o globals.o inflate.o list.o match.o
OBJS3 = process.o ttyio.o ubz2err.o unreduce.o unshrink.o zipinfo.o
OBJS4 = _sprintf.o _fprintf.o _isatty.o _stat.o _setargv.o
OBJS = $(OBJS1) $(OBJS2) $(OBJS3) $(OBJS4) theos.o
#LOBJS = $(OBJS)
LOBJS = $(OBJS1) $(OBJS2) $(OBJS3) theos.o
OBJX = unzipsfx.o crc32_.o $(CRCA_O) crypt_.o extract_.o fileio_.o \
	globals_.o inflate_.o match_.o process_.o ttyio_.o obz2err_.o theos_.o
LOBJX = $(OBJX)
OBJF = funzip.o crc32f.o $(CRCA_O) cryptf.o globalsf.o inflatef.o ttyiof.o \
	_sprintf.o _fprintf.o _isatty.o
UNZIP_H = unzip.h unzpriv.h globals.h $(OSDEP_H)
EBCDIC_H = ebcdic.h theos/charconv.h

UNZIPS = unzip.command funzip.command unzipsfx.command zipinfo.command

unzips:	$(UNZIPS)


crc32.o:	crc32.c $(UNZIP_H) zip.h crc32.h
crypt.o:	crypt.c $(UNZIP_H) zip.h crypt.h crc32.h ttyio.h
envargs.o:	envargs.c $(UNZIP_H)
explode.o:	explode.c $(UNZIP_H)
extract.o:	extract.c $(UNZIP_H) crc32.h crypt.h
fileio.o:	fileio.c $(UNZIP_H) crc32.h crypt.h ttyio.h EBCDIC_H
funzip.o:	funzip.c $(UNZIP_H) crc32.h crypt.h ttyio.h
globals.o:	globals.c $(UNZIP_H)
inflate.o:	inflate.c inflate.h $(UNZIP_H)
list.o:		list.c $(UNZIP_H)
match.o:	match.c $(UNZIP_H)
process.o:	process.c $(UNZIP_H) crc32.h
ttyio.o:	ttyio.c $(UNZIP_H) zip.h crypt.h ttyio.h
ubz2err.o:	ubz2err.c $(UNZIP_H)
unreduce.o:	unreduce.c $(UNZIP_H)
unshrink.o:	unshrink.c $(UNZIP_H)
unzip.o:	unzip.c $(UNZIP_H) crypt.h unzvers.h consts.h
zipinfo.o:	zipinfo.c $(UNZIP_H)

# unzipsfx only
crc32_.o:	crc32.c $(UNZIP_H) zip.h crc32.h
	$(CC) $(CF) -DSFX -Fo$@ crc32.c

crypt_.o:	crypt.c $(UNZIP_H) zip.h crypt.h crc32.h ttyio.h
	$(CC) $(CF) -DSFX -Fo$@ crypt.c

extract_.o:	extract.c $(UNZIP_H) crc32.h crypt.h
	$(CC) $(CF) -DSFX -Fo$@ extract.c

fileio_.o:	fileio.c $(UNZIP_H) crc32.h crypt.h ttyio.h EBCDIC_H
	$(CC) $(CF) -DSFX -Fo$@ fileio.c

globals_.o:	globals.c $(UNZIP_H)
	$(CC) $(CF) -DSFX -Fo$@ globals.c

inflate_.o:	inflate.c inflate.h $(UNZIP_H) crypt.h
	$(CC) $(CF) -DSFX -Fo$@ inflate.c

match_.o:	match.c $(UNZIP_H)
	$(CC) $(CF) -DSFX -Fo$@ match.c

process_.o:	process.c $(UNZIP_H) crc32.h
	$(CC) $(CF) -DSFX -Fo$@ process.c

ttyio_.o:	ttyio.c $(UNZIP_H) zip.h crypt.h ttyio.h
	$(CC) $(CF) -DSFX -Fo$@ ttyio.c

ubz2err_.o:	ubz2err.c $(UNZIP_H)
	$(CC) $(CF) -DSFX -Fo$@ ubz2err.c

unzipsfx.o:	unzip.c $(UNZIP_H) crypt.h unzvers.h consts.h
	$(CC) $(CF) -DSFX -Fo$@ unzip.c

# funzip only
crc32f.o:	crc32.c $(UNZIP_H) zip.h crc32.h
	$(CC) $(CF) -DFUNZIP -Fo$@ crc32.c

cryptf.o:	crypt.c $(UNZIP_H) zip.h crypt.h crc32.h ttyio.h
	$(CC) $(CF) -DFUNZIP -Fo$@ crypt.c

globalsf.o:	globals.c $(UNZIP_H)
	$(CC) $(CF) -DFUNZIP -Fo$@ globals.c

inflatef.o:	inflate.c inflate.h $(UNZIP_H) crypt.h
	$(CC) $(CF) -DFUNZIP -Fo$@ inflate.c

ttyiof.o:	ttyio.c $(UNZIP_H) zip.h crypt.h ttyio.h
	$(CC) $(CF) -DFUNZIP -Fo$@ ttyio.c

theos.o:	theos/theos.c $(UNZIP_H) unzvers.h		# THEOS only
	$(CC) $(CF) theos/theos.c

theos_.o:	theos/theos.c $(UNZIP_H)			# THEOS unzipsfx
	$(CC) $(CF) -DSFX -Fo$@ theos/theos.c

_sprintf.o:	theos/_sprintf.c $(UNZIP_H)			# THEOS only
	$(CC) $(CF) theos/_sprintf.c

_fprintf.o:	theos/_fprintf.c $(UNZIP_H)			# THEOS only
	$(CC) $(CF) theos/_fprintf.c

_isatty.o:	theos/_isatty.c $(UNZIP_H)			# THEOS only
	$(CC) $(CF) theos/_isatty.c

_stat.o:	theos/_stat.c $(UNZIP_H)			# THEOS only
	$(CC) $(CF) theos/_stat.c

_setargv.o:	theos/_setargv.c $(UNZIP_H)			# THEOS only
	$(CC) $(CF) theos/_setargv.c

unzip.command:	$(OBJS)
	$(LD) $(LF) $@ $(LOBJS) $(LF2)

unzipsfx.command:	$(OBJX)
	$(LD) $(LF) $@ $(LOBJX) $(LF2)

funzip.command:	$(OBJF)
	$(LD) $(LF) $@ $(OBJF) $(LF2)

zipinfo.command:	unzip.command
	$(LN) $? $@ ( rep not

install:
	copy *.command /system.cmd32.=(rep noq

clean:
	erase *.o(noq not
	erase *.command(noq not
