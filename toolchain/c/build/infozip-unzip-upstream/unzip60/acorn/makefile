# Makefile for UnZip, UnZipSFX, ZipInfo and fUnZip (5.53 or later)
# last modified:  25 Dec 2006

# add -g to CC to debug
# add -d to BIND to debug
CC   = cc
BIND = link
AS   = $(CC) -c
ASM  = objasm
SQUEEZE = squeeze -v
E    =

# flags
#   CFLAGS    flags for C compile
#   LFLAGS1   flags after output file spec, before obj file list
#   LFLAGS2   flags after obj file list (libraries, etc)
#
LIB      =
CBASE    = -throwback -wn -DNO_STRNICMP
CFLAGS   = $(CBASE) -IC:,@.
ASMFLAGS = -Throwback -Stamp -NoCache -CloseExec -quit
LFLAGS1  =
LFLAGS2  = $(LIB) C:o.Stubs

# object file lists
OBJS1 = unzip.o crc32.o crypt.o envargs.o explode.o
OBJS2 = extract.o fileio.o globals.o inflate.o list.o match.o
OBJS3 = process.o ttyio.o ubz2err.o unreduce.o unshrink.o zipinfo.o
OBJS  = $(OBJS1) $(OBJS2) $(OBJS3) riscos.o swiven.o acorn.o
OBJF  = funzip.o crc32.o cryptf.o globalsf.o inflatef.o ttyiof.o \
        riscos.o swiven.o
OBJX  = unzipsfx.o crc32.o crypt_.o extract_.o fileio_.o globals.o \
        inflate.o match.o process_.o ttyio.o ubz2err_.o \
        acorn_.o swiven.o riscos_.o

UNZIP_H = unzip.h unzpriv.h globals.h acorn/riscos.h acorn/swiven.h

all:      unzip funzip unzipsfx


install:  unzip funzip unzipsfx
        $(SQUEEZE) unzip %.unzip
        $(SQUEEZE) funzip %.funzip
        $(SQUEEZE) unzipsfx unzipsfx
        copy %.unzip %.zipinfo ~CVF

# suffix rules
.SUFFIXES:      .o .c
.c.o:
        $(CC) $(CFLAGS) -c $<
.s.o:
        $(ASM) $(ASMFLAGS) -from @*.s -to @*.o

# rules for unzip and funzip
crc32.o:        crc32.c $(UNZIP_H) zip.h crc32.h
crypt.o:        crypt.c $(UNZIP_H) zip.h crypt.h crc32.h ttyio.h
envargs.o:      envargs.c $(UNZIP_H)
explode.o:      explode.c $(UNZIP_H)
extract.o:      extract.c $(UNZIP_H) crc32.h crypt.h
fileio.o:       fileio.c $(UNZIP_H) crc32.h crypt.h ttyio.h ebcdic.h
funzip.o:       funzip.c $(UNZIP_H) crc32.h crypt.h ttyio.h
globals.o:      globals.c $(UNZIP_H)
inflate.o:      inflate.c inflate.h $(UNZIP_H)
list.o:         list.c $(UNZIP_H)
match.o:        match.c $(UNZIP_H)
process.o:      process.c $(UNZIP_H) crc32.h
ttyio.o:        ttyio.c $(UNZIP_H) zip.h crypt.h ttyio.h
ubz2err.o:      ubz2err.c $(UNZIP_H)
unreduce.o:     unreduce.c $(UNZIP_H)
unshrink.o:     unshrink.c $(UNZIP_H)
unzip.o:        unzip.c $(UNZIP_H) crypt.h unzvers.h consts.h
zipinfo.o:      zipinfo.c $(UNZIP_H)

crypt_.o:       crypt.c $(UNZIP_H) zip.h crypt.h crc32.h ttyio.h
        $(CC) $(CFLAGS) -DSFX -c c.crypt -o o.crypt_
extract_.o:     extract.c $(UNZIP_H) crc32.h crypt.h
        $(CC) $(CFLAGS) -DSFX -c c.extract -o o.extract_
fileio_.o:      fileio.c $(UNZIP_H) crc32.h crypt.h
        $(CC) $(CFLAGS) -DSFX -c c.fileio -o o.fileio_
globals_.o:     globals.c $(UNZIP_H)
        $(CC) $(CFLAGS) -DFUNZIP -c c.globals -o o.globals_
inflate_.o:     inflate.c inflate.h $(UNZIP_H) crypt.h
        $(CC) $(CFLAGS) -DFUNZIP -c c.inflate -o o.inflate_
process_.o:     process.c $(UNZIP_H) crc32.h
        $(CC) $(CFLAGS) -DSFX -c c.process -o o.process_
ttyio_.o:       ttyio.c $(UNZIP_H) zip.h crypt.h ttyio.h
        $(CC) $(CFLAGS) -DFUNZIP -c c.ttyio -o o.ttyio_
ubz2err_.o:     ubz2err.c $(UNZIP_H)
        $(CC) $(CFLAGS) -DSFX -c c.ubz2err -o o.ubz2err_

unzipsfx.o:     unzip.c $(UNZIP_H) crypt.h unzvers.h consts.h
        $(CC) $(CFLAGS) -DSFX -c c.unzip -o o.unzipsfx

o.cryptf:       c.crypt $(UNZIP_H) h.zip h.crypt h.crc32 h.ttyio
        $(CC) $(CFLAGS) -DFUNZIP -c c.crypt -o o.cryptf
o.globalsf:     c.globals $(UNZIP_H)
        $(CC) $(CFLAGS) -DFUNZIP -c c.globals -o o.globalsf
o.inflatef:     c.inflate h.inflate $(UNZIP_H) h.crypt
        $(CC) $(CFLAGS) -DFUNZIP -c c.inflate -o o.inflatef
o.ttyiof:       c.ttyio $(UNZIP_H) h.zip h.crypt h.ttyio
        $(CC) $(CFLAGS) -DFUNZIP -c c.ttyio -o o.ttyiof

acorn.o:       acorn/acorn.c $(UNZIP_H)
        $(CC) $(CFLAGS) -c acorn/acorn.c
acorn_.o:       acorn/acorn.c $(UNZIP_H)
        $(CC) $(CFLAGS) -c -DSFX -DSFX_EXDIR -o acorn_.o acorn/acorn.c

riscos.o:       acorn/riscos.c $(UNZIP_H)
        $(CC) $(CFLAGS) -c acorn/riscos.c
riscos_.o:      acorn/riscos.c $(UNZIP_H)
        $(CC) $(CFLAGS) -c -DSFX -DSFX_EXDIR -o riscos_.o acorn/riscos.c

swiven.o:       acorn/swiven.s
        $(ASM) $(ASMFLAGS) -from acorn.s.swiven -to o.swiven

unzip:    $(OBJS)
          $(BIND) -o unzip$(E) $(LFLAGS1) $(OBJS) $(LFLAGS2)
funzip:   $(OBJF)
          $(BIND) -o funzip$(E) $(LFLAGS1) $(OBJF) $(LFLAGS2)
unzipsfx: $(OBJX)
          $(BIND) -o unzipsfx$(E) $(LFLAGS1) $(OBJX) $(LFLAGS2)

clean:  ;remove unzip; remove funzip;
        remove zipinfo; remove unzipsfx;
        create o.!fake! 0
        wipe o.* ~cf

# end of Makefile
