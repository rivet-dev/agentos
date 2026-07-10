#==============================================================================
# Makefile for UnZip, UnZipSFX and fUnZip:  Atari           ("real" makes only)
# Version:  5.53                                               25 December 2006
#==============================================================================


# INSTRUCTIONS (such as they are):
#
# "make"	-- makes UnZip on a generic Atari
#
# CF are flags for the C compiler.  LF are flags for the loader.  LF2 are more
# flags for the loader, if they need to be at the end of the line instead of at
# the beginning (for example, some libraries).  FL and FL2 are the corre-
# sponding flags for fUnZip.  LOCAL_UNZIP is an environment variable that can
# be used to add default C flags to your compile without editing the Makefile
# (e.g., -DDEBUG_STRUC, or -FPi87 on PCs using Microsoft C).
#
# Be sure to test your new UnZip (and UnZipSFX and fUnZip); successful compila-
# tion does not always imply a working program.


#####################
# MACRO DEFINITIONS #
#####################

# Defaults most systems use (use LOCAL_UNZIP in environment to add flags,
# such as -DDOSWILD).

# UnZip flags
CC = gcc#	try using "gcc" target rather than changing this (if you do,
LD = $(CC)#	you MUST change LD, too--else "unresolved symbol:  ___main")
LOC = $(LOCAL_UNZIP)
CF = $(CFLAGS) $(LOC)
LF = -o unzip$E
LF2 = -s

# UnZipSFX flags
SL = -o unzipsfx$E
SL2 = $(LF2)

# fUnZip flags
FL = -o funzip$E
FL2 = $(LF2)

# general-purpose stuff
CP = ln -s
LN = ln
RM = rm -f
CHMOD = chmod
STRIP = strip
E = .ttp
O = .o
M = atari
SHELL = /bin/sh

# object files
OBJS1 = unzip$O crc32$O crypt$O envargs$O explode$O
OBJS2 = extract$O fileio$O globals$O inflate$O list$O match$O
OBJS3 = process$O ttyio$O ubz2err$O unreduce$O unshrink$O zipinfo$O
OBJS = $(OBJS1) $(OBJS2) $(OBJS3) $M$O
LOBJS = $(OBJS)
OBJSDLL = $(OBJS) api$O
OBJX = unzipsfx$O crc32$O crypt_$O extract_$O fileio_$O \
	globals_$O inflate_$O match_$O process_$O ttyio_$O ubz2err_$O $M_$O
LOBJX = $(OBJX)
OBJF = funzip$O crc32$O cryptf$O globalsf$O inflatef$O ttyiof$O
#OBJS_OS2 = $(OBJS1:.o=.obj) $(OBJS2:.o=.obj) os2.obj
#OBJF_OS2 = $(OBJF:.o=.obj)

UNZIP_H = unzip.h unzpriv.h globals.h

# installation
INSTALL = cp#	probably can change this to 'install' if you have it
# on some systems, manext=l and MANDIR=/usr/man/man$(manext) may be appropriate
manext = 1
prefix = /usr/local
BINDIR = $(prefix)/bin#			where to install executables
MANDIR = $(prefix)/man/man$(manext)#	where to install man pages
INSTALLEDBIN = $(BINDIR)/funzip$E $(BINDIR)/zipinfo$E $(BINDIR)/unzipsfx$E \
	$(BINDIR)/unzip$E
INSTALLEDMAN = $(MANDIR)/unzip.$(manext) $(MANDIR)/funzip.$(manext) \
	$(MANDIR)/unzipsfx.$(manext) $(MANDIR)/zipinfo.$(manext)
#
UNZIPS = unzip$E funzip$E unzipsfx$E
# this is a little ugly...well, no, it's a lot ugly:
MANS = man/unzip.1 man/unzipsfx.1 man/zipinfo.1 man/funzip.1
DOCS = unzip.txt unzipsfx.txt zipinfo.txt funzip.txt


###############################################
# BASIC COMPILE INSTRUCTIONS AND DEPENDENCIES #
###############################################

# this is for GNU make; comment out and notify zip-bugs if it causes errors
.SUFFIXES:	.c .o .obj

# yes, we should be able to use the $O macro to combine these two, but it
# fails on some brain-damaged makes (e.g., AIX's)...no big deal
.c.o:
	$(CC) -c $(CF) $*.c

.c.obj:
	$(CC) -c $(CF) $*.c


####################
# DEFAULT HANDLING #
####################


all:		unzips
unzips:		$(UNZIPS)
docs:		$(DOCS)
unzipsman:	unzips docs
unzipsdocs:	unzips docs


unzip$E:	$(OBJS)
	$(LD) $(LF) $(LOBJS) $(LF2)

unzipsfx$E:	$(OBJX)
	$(LD) $(SL) $(LOBJX) $(SL2)

funzip$E:	$(OBJF)
	$(LD) $(FL) $(OBJF) $(FL2)


crc32$O:	crc32.c $(UNZIP_H) zip.h crc32.h
crypt$O:	crypt.c $(UNZIP_H) zip.h crypt.h crc32.h ttyio.h
envargs$O:	envargs.c $(UNZIP_H)
explode$O:	explode.c $(UNZIP_H)
extract$O:	extract.c $(UNZIP_H) crc32.h crypt.h
fileio$O:	fileio.c $(UNZIP_H) crc32.h crypt.h ttyio.h ebcdic.h
funzip$O:	funzip.c $(UNZIP_H) crc32.h crypt.h ttyio.h
globals$O:	globals.c $(UNZIP_H)
inflate$O:	inflate.c inflate.h $(UNZIP_H)
list$O:		list.c $(UNZIP_H)
match$O:	match.c $(UNZIP_H)
process$O:	process.c $(UNZIP_H) crc32.h
ttyio$O:	ttyio.c $(UNZIP_H) zip.h crypt.h ttyio.h
ubz2err$O:	ubz2err.c $(UNZIP_H)
unreduce$O:	unreduce.c $(UNZIP_H)
unshrink$O:	unshrink.c $(UNZIP_H)
unzip$O:	unzip.c $(UNZIP_H) crypt.h unzvers.h consts.h
zipinfo$O:	zipinfo.c $(UNZIP_H)

# unzipsfx only
crypt_$O:	crypt.c $(UNZIP_H) zip.h crypt.h crc32.h ttyio.h
	$(CP) crypt.c crypt_.c
	$(CC) -c $(CF) -DSFX crypt_.c
	$(RM) crypt_.c

extract_$O:	extract.c $(UNZIP_H) crc32.h crypt.h
	$(CP) extract.c extract_.c
	$(CC) -c $(CF) -DSFX extract_.c
	$(RM) extract_.c

fileio_$O:	fileio.c $(UNZIP_H) crc32.h crypt.h ttyio.h ebcdic.h
	$(CP) fileio.c fileio_.c
	$(CC) -c $(CF) -DSFX fileio_.c
	$(RM) fileio_.c

globals_$O:	globals.c $(UNZIP_H)
	$(CP) globals.c globals_.c
	$(CC) -c $(CF) -DSFX globals_.c
	$(RM) globals_.c

inflate_$O:	inflate.c inflate.h $(UNZIP_H) crypt.h
	$(CP) inflate.c inflate_.c
	$(CC) -c $(CF) -DSFX inflate_.c
	$(RM) inflate_.c

match_$O:	match.c $(UNZIP_H)
	$(CP) match.c match_.c
	$(CC) -c $(CF) -DSFX match_.c
	$(RM) match_.c

process_$O:	process.c $(UNZIP_H) crc32.h
	$(CP) process.c process_.c
	$(CC) -c $(CF) -DSFX process_.c
	$(RM) process_.c

ttyio_$O:	ttyio.c $(UNZIP_H) zip.h crypt.h ttyio.h
	$(CP) ttyio.c ttyio_.c
	$(CC) -c $(CF) -DSFX ttyio_.c
	$(RM) ttyio_.c

ubz2err$O:	ubz2err.c $(UNZIP_H)
	$(CP) ubz2err.c ubz2err_.c
	$(CC) -c $(CF) -DSFX ubz2err_.c
	$(RM) ubz2err_.c

unzipsfx$O:	unzip.c $(UNZIP_H) crypt.h unzvers.h consts.h
	$(CP) unzip.c unzipsfx.c
	$(CC) -c $(CF) -DSFX unzipsfx.c
	$(RM) unzipsfx.c


# funzip only
cryptf$O:	crypt.c $(UNZIP_H) zip.h crypt.h crc32.h ttyio.h
	$(CP) crypt.c cryptf.c
	$(CC) -c $(CF) -DFUNZIP cryptf.c
	$(RM) cryptf.c

globalsf$O:	globals.c $(UNZIP_H)
	$(CP) globals.c globalsf.c
	$(CC) -c $(CF) -DFUNZIP globalsf.c
	$(RM) globalsf.c

inflatef$O:	inflate.c inflate.h $(UNZIP_H) crypt.h
	$(CP) inflate.c inflatef.c
	$(CC) -c $(CF) -DFUNZIP inflatef.c
	$(RM) inflatef.c

ttyiof$O:	ttyio.c $(UNZIP_H) zip.h crypt.h ttyio.h
	$(CP) ttyio.c ttyiof.c
	$(CC) -c $(CF) -DFUNZIP ttyiof.c
	$(RM) ttyiof.c


# system-specific code
atari$O:	atari/atari.c $(UNZIP_H)
	$(CC) -c $(CF) atari/atari.c

atari_$O:	atari/atari.c $(UNZIP_H)
	$(CP) atari/atari.c atari_.c
	$(CC) -c $(CF) -DSFX atari_.c
	$(RM) atari_.c


# this really only works for Unix targets, unless specify E and O on cmd line
clean:
	@echo ""
	@echo '         This is a Unix-specific target.  (Just so you know.)'
	@echo ""
	rm -f $(OBJS) api$O apihelp$O unzipstb$O $(OBJF) $(OBJX) $(UNZIPS)

install:	$(UNZIPS) $(MANS)
	$(INSTALL) $(UNZIPS) $(BINDIR)
	$(RM) $(BINDIR)/zipinfo$E
	$(LN) $(BINDIR)/unzip$E $(BINDIR)/zipinfo$E
	$(INSTALL) man/unzip.1 $(MANDIR)/unzip.$(manext)
	$(INSTALL) man/unzipsfx.1 $(MANDIR)/unzipsfx.$(manext)
	$(INSTALL) man/zipinfo.1 $(MANDIR)/zipinfo.$(manext)
	$(INSTALL) man/funzip.1 $(MANDIR)/funzip.$(manext)
	$(CHMOD) 755  $(INSTALLEDBIN)
	$(CHMOD) 644  $(INSTALLEDMAN)

# alternatively, could use zip method:  -cd $(BINDIR); rm -f $(UNZIPS)  [etc.]
uninstall:
	rm -f $(INSTALLEDBIN) $(INSTALLEDMAN)


TESTZIP = testmake.zip	# the test zipfile

# test some basic features of the build
test:		check

check:	unzips
	@echo '  This is a Unix-specific target.  (Just so you know.)'
	if test ! -f $(TESTZIP); then \
		echo "  error:  can't find test file $(TESTZIP)"; exit 1; fi
#
	echo "  testing extraction"
	./unzip -b $(TESTZIP) testmake.zipinfo
	if test $? ; then \
	    echo "  error:  file extraction from $(TESTZIP) failed"; exit 1; fi
#
	echo '  testing zipinfo (unzip -Z)'
	./unzip -Z $(TESTZIP) > testmake.unzip-Z
	if diff testmake.unzip-Z testmake.zipinfo; then ;; else \
	    echo '  error:  zipinfo output doesn't match stored version'; fi
	$(RM) testmake.unzip-Z testmake.zipinfo
#
	echo '  testing unzip -d exdir option'
	./unzip -b $(TESTZIP) -d testun
	cat testun/notes
#
	echo '  testing unzip -o and funzip (ignore funzip warning)'
	./unzip -boq $(TESTZIP) notes -d testun
	./funzip < $(TESTZIP) > testun/notes2
	if diff testun/notes testun/notes2; then ;; else \
	    echo 'error:  funzip output disagrees with unzip'; fi
#
	echo '  testing unzipsfx (self-extractor)'
	cat unzipsfx $(TESTZIP) > testsfx
	$(CHMOD) 0700 testsfx
	./testsfx -b notes
	if diff notes testun/notes; then ;; else \
	    echo '  error:  unzipsfx file disagrees with unzip'; fi
	$(RM) testsfx notes testun/notes testun/notes2
	rmdir testun
#
	echo '  testing complete.'


################################
# INDIVIDUAL MACHINE MAKERULES #
################################

# these are left over for backward compatibility/convenience

generic:	unzips
atari:		unzips
