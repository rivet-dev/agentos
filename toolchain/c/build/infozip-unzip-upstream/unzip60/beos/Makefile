######################################################################
#
# Makefile for Info-ZIP's unzip, unzipsfx, and funzip on BeOS
#
# Copyright (c) 1998-2006 Info-ZIP
#             Chris Herborth (chrish@qnx.com)
#
# This is the new New and Improved Makefile for BeOS; it will:
#
# 1) automatically detect your platform (PowerPC or x86) if none is
#    specified; the default compiler is CodeWarrior for PowerPC, or
#    gcc for x86
#
# 2) let you cross-compile for the other platform (PowerPC or x86), in
#    theory
#
# 3) let you use Metrowerks CodeWarrior (default) or GNU C to build with
#    for either platfor, in theory
#
# To choose a specific architecture, define the ARCH environment
# variable on the make command-line:
#
# ARCH=what make -f beos/Makefile
#
# where "what" can be "powerpc" or "x86".
#
# To choose a specific compiler, define the CC environment variable on
# the make command-line:
#
# CC=compiler make -f beos/Makefile
#
# where "compiler" can be "mwcc" or "x86".
#
# Of course, you can combine these two:
#
# ARCH=powerpc CC=mwcc make -f beos/Makefile
#
# or:
#
# CC=gcc ARCH=x86 make -f beos/Makefile
#
# To automatically install the fresh new unzip, use the "install" target:
#
# make -f beos/Makefile install

######################################################################
# Things that don't change:

SHELL = /bin/sh

# Punish those of you not running on SMP hardware...
MAKE  = make -j 4 -f beos/Makefile

LOC=$(LOCAL_UNZIP) -DPASSWD_FROM_STDIN
AF=$(LOC)

# UnZipSfx flags
SL = -o unzipsfx
SL2 = $(LF2)

# fUnZip flags
FL = -o funzip
FL2 = $(LF2)

# general-purpose stuff
CP = cp
RM = rm -f
LN = ln -sf
E =
O = .o
M = beos

# defaults for crc32 stuff and system-dependent headers
CRCA_O =
OSDEP_H = beos/beocfg.h

# object files
OBJS1 = unzip$O crc32$O crypt$O envargs$O explode$O
OBJS2 = extract$O fileio$O globals$O inflate$O list$O match$O
OBJS3 = process$O ttyio$O ubz2err$O unreduce$O unshrink$O zipinfo$O
OBJS = $(OBJS1) $(OBJS2) $(OBJS3) $M$O $(BEOS_MAIN)
LOBJS = $(OBJS)
OBJSDLL = $(OBJS) api$O
OBJX = unzipsfx$O crc32_$O crypt_$O extract_$O fileio_$O globals_$O \
	inflate_$O match_$O process_$O ttyio_$O ubz2err_$O $M_$O $(BEOS_MAIN)
LOBJX = $(OBJX)
OBJF = funzip$O crc32f$O cryptf$O globalsf$O inflatef$O ttyiof$O
UNZIP_H = unzip.h unzpriv.h globals.h $(OSDEP_H)

# installation
INSTALL = install
# on some systems, manext=l and MANDIR=/usr/man/man$(manext) may be appropriate
manext = 1
prefix = /boot/home/config
BINDIR = $(prefix)/bin#			where to install executables
MANDIR = $(prefix)/man/man$(manext)#	where to install man pages
INSTALLEDBIN = $(BINDIR)/funzip$E $(BINDIR)/zipinfo$E $(BINDIR)/unzipsfx$E \
	$(BINDIR)/unzip$E
INSTALLEDMAN = $(MANDIR)/unzip.$(manext) $(MANDIR)/funzip.$(manext) \
	$(MANDIR)/unzipsfx.$(manext) $(MANDIR)/zipinfo.$(manext)
#
UNZIPS = unzip$E funzip$E unzipsfx$E zipinfo$E
# this is a little ugly...well, no, it's a lot ugly:
MANS = man/unzip.1 man/unzipsfx.1 man/zipinfo.1 man/funzip.1 man/zipgrep.1
DOCS = unzip.txt unzipsfx.txt zipinfo.txt funzip.txt zipgrep.txt

######################################################################
# Things that change:

# Select an architecture:
ifndef ARCH
MACHINE=$(shell uname -m)
ifeq "$(MACHINE)" "BePC"
ARCH=x86
CC=gcc
else
ARCH=powerpc
CC=mwcc
endif
endif

# Now select compiler flags and whatnot based on the ARCH and CC:
WHAT=$(ARCH)-$(CC)

ifeq "$(WHAT)" "powerpc-mwcc"
CC=mwccppc
LD=mwccppc
CF=-w9 -O7 -opt schedule604 -rostr -I. $(LOC)
LF=-o unzip
LF2=-warn -L/boot/develop/lib/ppc -lbe -lroot
BEOS_MAIN=beosmain$O
TARGET=$(UNZIPS)
endif

ifeq "$(WHAT)" "powerpc-gcc"
CC=gcc
LD=gcc
CF=-O3 -mcpu=604 -Wall -ansi -I. -I/boot/develop/headers/be/support \
   -I/boot/develop/headers/be/storage $(LOC)
LF=-o unzip
LF2=-L/boot/develop/lib/ppc -lbe -lroot
BEOS_MAIN=beosmain$O
TARGET=$(UNZIPS)
endif

# This isn't likely to happen for R4 or later...
ifeq "$(WHAT)" "x86-mwcc"
CC=mwccx86
LD=mwccx86
CF=-O2 -w9 -I. $(LOC)
LF=-o unzip
LF2=-warn -L/boot/develop/lib/x86 -lbe -lroot
BEOS_MAIN=beosmain$O
TARGET=$(UNZIPS)
endif

ifeq "$(WHAT)" "x86-gcc"
CC=gcc
LD=gcc
CF=-O3 -mpentiumpro \
   -Wall -Wno-multichar -Wno-trigraphs \
   -ansi -I. -I/boot/develop/headers/be/support \
   -I/boot/develop/headers/be/storage $(LOC)
LF=-o unzip
LF2=-L/boot/develop/lib/x86 -lbe -lroot
BEOS_MAIN=beosmain$O
TARGET=$(UNZIPS)
endif

ifndef TARGET
TARGET=help
endif

######################################################################
# Helpful targets
all:
	@echo 'TARGET = $(TARGET)'
	@echo 'ARCH   = $(ARCH)'
	@echo 'CC     = $(CC)'
	if [ -n "$(TARGET)" ] ; then \
		$(MAKE) CC=$(CC) CF="$(CF)" LD="$(LD)" \
			LF="$(LF)" LF2="$(LF2)" CCPP="$(CC)" CPPF="$(CF)" \
			OBJS="$(OBJS)" LOBJS="$(LOBJS)" OBJX="$(OBJX)" \
			LOBJX="$(LOBJX)" $(TARGET) ; \
	else \
		$(MAKE) help ; \
	fi

help:
	@echo ''
	@echo "This Makefile lets you build Info-ZIP's zip."
	@echo ''
	@echo 'To build zip for this computer using the default compiler, just do:'
	@echo ''
	@echo '	make -f beos/Makefile'
	@echo ''
	@echo 'To build zip for a specific architecture using a specific'
	@echo 'compiler, do:'
	@echo ''
	@echo '	ARCH=cpu CC=compiler make -f beos/Makefile'
	@echo ''
	@echo 'Where:'
	@echo '	cpu is either "powerpc" or "x86"'
	@echo '	compiler is either "mwcc" or "gcc"'
	@echo ''

######################################################################
# Basic compile instructions and dependencies

# this is for GNU make; comment out and notify zip-bugs if it causes errors
.SUFFIXES:	.c .o

# default for compiling C files
.c.o:
	$(CC) -c $(CF) $*.c


unzips:		$(UNZIPS)
objs:		$(OBJS)
objsdll:	$(OBJSDLL)
docs:		$(DOCS)
unzipsman:	unzips docs
unzipsdocs:	unzips docs


unzip$E:	$(OBJS) beos/unzip.rsc
	$(LD) $(LF) $(LOBJS) $(LF2)
	chmod +x unzip$E
	xres -o unzip$E beos/unzip.rsc
	mimeset -f -all unzip$E

unzipsfx$E:	$(OBJX) beos/unzipsfx.rsc
	$(LD) $(SL) $(LOBJX) $(SL2)
	chmod +x unzipsfx$E
	xres -o unzipsfx$E beos/unzipsfx.rsc
	mimeset -f -all unzipsfx

funzip$E:	$(OBJF)
	$(LD) $(FL) $(OBJF) $(FL2)
	chmod +x funzip$E

zipinfo$E:	unzip$E
	$(LN) unzip$E zipinfo$E


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
process$O:	process.c $(UNZIP_H)
ttyio$O:	ttyio.c $(UNZIP_H) zip.h crypt.h ttyio.h
ubz2err$O:	ubz2err.c $(UNZIP_H)
unreduce$O:	unreduce.c $(UNZIP_H)
unshrink$O:	unshrink.c $(UNZIP_H)
unzip$O:	unzip.c $(UNZIP_H) crypt.h unzvers.h consts.h
zipinfo$O:	zipinfo.c $(UNZIP_H)

# unzipsfx compilation section
unzipsfx$O:	unzip.c $(UNZIP_H) crypt.h unzvers.h consts.h
	$(CP) unzip.c unzipsfx.c
	$(CC) -c $(CF) -DSFX unzipsfx.c
	$(RM) unzipsfx.c

crc32_$O:	crc32.c $(UNZIP_H) zip.h crc32.h
	$(CP) crc32.c crc32_.c
	$(CC) -c $(CF) -DSFX crc32_.c
	$(RM) crc32_.c

crypt_$O:	crypt.c $(UNZIP_H) zip.h crypt.h ttyio.h
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

# funzip compilation section
crc32f$O:	crc32.c $(UNZIP_H) zip.h crc32.h
	$(CP) crc32.c crc32f.c
	$(CC) -c $(CF) -DFUNZIP crc32f.c
	$(RM) crc32f.c

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

# BeOS specific code
beos$O:		beos/beos.c $(UNZIP_H) unzvers.h beos/beos.h
	$(CC) -c $(CF) beos/beos.c

beosmain$O:	beos/beosmain.cpp $(UNZIP_H) unzvers.h
	$(CCPP) -c $(CPPF) beos/beosmain.cpp

# version() not used by unzipsfx, so no unzvers.h dependency
beos_$O:	beos/beos.c $(UNZIP_H) beos/beos.h		# unzipsfx only
	$(CP) beos/beos.c beos_.c
	$(CC) -c $(CF) -Ibeos -DSFX beos_.c
	$(RM) beos_.c

beosmain_$O:	beos/beosmain.cpp $(UNZIP_H)			# unzipsfx only
	$(CP) beos/beosmain.cpp beosmain_.cpp
	$(CCPP) -c $(CPPF) -Ibeos -DSFX beosmain_.cpp
	$(RM) beosmain_.cpp


# this really only works for Unix targets, unless E and O specified on cmd line
clean:
	-rm -f $(UNZIPS) $(OBJS) $(OBJF) $(OBJX) api$O apihelp$O crc_gcc$O \
	  unzipstb$O beosmain.o beosmain_.o

install:	all
	$(INSTALL) -m 755 $(UNZIPS) $(BINDIR)
	mimeset -f -all $(BINDIR)/unzip
	mimeset -f -all $(BINDIR)/unzipsfx
	$(RM) $(BINDIR)/zipinfo$E
	$(LN) unzip$E $(BINDIR)/zipinfo$E
	$(RM) $(BINDIR)/zipgrep$E
	$(INSTALL) -m 755 unix/zipgrep $(BINDIR)/zipgrep$E
	$(INSTALL) -m 644 man/unzip.1 $(MANDIR)/unzip.$(manext)
	$(INSTALL) -m 644 man/unzipsfx.1 $(MANDIR)/unzipsfx.$(manext)
	$(INSTALL) -m 644 man/zipinfo.1 $(MANDIR)/zipinfo.$(manext)
	$(INSTALL) -m 644 man/funzip.1 $(MANDIR)/funzip.$(manext)
	$(INSTALL) -m 644 man/zipgrep.1 $(MANDIR)/zipgrep.$(manext)
	$(INSTALL) -m 644 $(DOCS) $(MANDIR)

# alternatively, could use zip method:  -cd $(BINDIR); rm -f $(UNZIPS)  [etc.]
uninstall:
	rm -f $(INSTALLEDBIN) $(INSTALLEDMAN)


TESTZIP = testmake.zip	# the test zipfile

# test some basic features of the build
test:		check

check:	unzips
	@echo '  This is a Unix-specific target.  (Just so you know.)'
	@echo '  (Should work ok on BeOS... [cjh])'
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
