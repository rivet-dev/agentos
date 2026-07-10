# Makefile for UnZip's bzip2 support library
# MSDOS & Win32 ports, using OpenWatcom C++
#
# (c) 2006-2007 Info-ZIP
# Last revision: Christian Spieler, 2007-Apr-03
#
# This Makefile is intended to be called from UnZip's main make procedure.

CC=wcc386
AR=lib386
CFLSYS = -6r -zt -zq -wx -s -obhikl+rt -oe100 -zp8 -q
CFLAGS= $(CFLSYS) -DBZ_NO_STDIO
!ifndef BZROOTDIR
BZROOTDIR=.
!endif
BZROOT=$(BZROOTDIR)\
!ifndef BZOBDIR
BZOBDIR=.
!endif
BZOB=$(BZOBDIR)/
O=.obj


OBJS= $(BZOB)blocksort$(O)  &
      $(BZOB)huffman$(O)    &
      $(BZOB)crctable$(O)   &
      $(BZOB)randtable$(O)  &
      $(BZOB)compress$(O)   &
      $(BZOB)decompress$(O) &
      $(BZOB)bzlib$(O)

izlib: $(BZOBDIR) $(BZOB)bz2.lib

$(BZOBDIR) :
	-mkdir $@

$(BZOB)bz2.lib: $(OBJS)
	$(AR) -nologo $(OBJS) -out:$(BZOB)bz2.lib

clean:  .SYMBOLIC
	-del $(BZOBDIR)\*$(O)
	-del $(BZOBDIR)\bz2.lib

.c$(O):
	$(CC) $(CFLAGS) -fo=$@ $<

$(OBJS) : $(BZROOT)bzlib.h $(BZROOT)bzlib_private.h

$(BZOB)blocksort$(O) : $(BZROOT)blocksort.c
	$(CC) $(CFLAGS) -Fo=$@ $(BZROOT)blocksort.c
$(BZOB)huffman$(O) : $(BZROOT)huffman.c
	$(CC) $(CFLAGS) -Fo=$@ $(BZROOT)huffman.c
$(BZOB)crctable$(O) : $(BZROOT)crctable.c
	$(CC) $(CFLAGS) -Fo=$@ $(BZROOT)crctable.c
$(BZOB)randtable$(O) : $(BZROOT)randtable.c
	$(CC) $(CFLAGS) -Fo=$@ $(BZROOT)randtable.c
$(BZOB)compress$(O) : $(BZROOT)compress.c
	$(CC) $(CFLAGS) -Fo=$@ $(BZROOT)compress.c
$(BZOB)decompress$(O) : $(BZROOT)decompress.c
	$(CC) $(CFLAGS) -Fo=$@ $(BZROOT)decompress.c
$(BZOB)bzlib$(O) : $(BZROOT)bzlib.c
	$(CC) $(CFLAGS) -Fo=$@ $(BZROOT)bzlib.c
