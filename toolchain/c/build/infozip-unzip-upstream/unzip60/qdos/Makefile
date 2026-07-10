# Makefile for UnZip 5.53+ using QDOS/xtc68 and GNU make
#
#      This makefile is a makefile for the xtc68 cross-compiler on Linux;
#      conversion to a native c68 makefile should be trivial. This builds
#      native QDOS binaries.
#
# Last modified:  25 Dec 06

include /etc/ql.mak
CFLAGS = -O -I. -I./qdos/

OBJS = unzip.o process.o crypt.o envargs.o explode.o \
         unshrink.o extract.o zipinfo.o inflate.o match.o unreduce.o \
         ubz2err.o ttyio.o fileio.o list.o globals.o qdos.o config.o crc32.o

FOBJS = funzip.o ttyio.o crypt.fo inflate.fo globals.fo config.o crc32.fo qdos.fo

XOBJS = unzip.xo process.xo crypt.xo extract.xo fileio.xo \
        ttyio.xo inflate.xo match.xo globals.xo ubz2err.xo \
        qdos.xo config.o crc32.xo

all : unzip unzipsfx funzip makesfx callstub

# xtc68/cc is brain dead !!

%.xo : %.c
	qcpp -DSFX -DSFX_EXDIR -I. -I./qdos/ $*.c $*.i
	c68 -O $*.i $*.s
	as68 $*.s $*.xo
	rm -f $*.s $*.i

%.fo : %.c
	qcpp -DFUNZIP -I. -I./qdos/ $*.c $*.i
	c68 -O $*.i $*.s
	as68 $*.s $*.fo
	rm -f $*.s $*.i

u : unzip

f : funzip

x : unzipsfx

m : makesfx

unzip :     $(OBJS)
	$(LD) $(LDFLAGS) -o$@ $(OBJS) $(LDLIBS)

unzipsfx :  $(XOBJS)
	$(LD) $(LDFLAGS) -o$@ $(XOBJS) $(LDLIBS)

funzip :    $(FOBJS)
	$(LD) $(LDFLAGS) -o$@ $(FOBJS) $(LDLIBS)

makesfx : qdos/makesfx.c
	cp qdos/makesfx.c .
	$(CC) $(CFLAGS) -o makesfx makesfx.c
	rm -f makesfx.c

callstub : qdos/callstub.c
	cp qdos/callstub.c .
	$(CC) -screspr.o $(CFLAGS) -o callstub callstub.c
	rm -f callstub.c

clean :
	rm *.o *.xo *.fo

cleaner : clean
	rm unzip funzip unzipsfx makesfx


$(OBJS) $(XOBJS) $(FOBJS) : unzip.h unzpriv.h globals.h \
                             qdos/izqdos.h

crc32.o crc32.fo crc32.xo : zip.h
crypt.o crypt.fo crypt.xo ttyio.o ttyio.fo ttyio.xo : zip.h

inflate.o inflate.fo inflate.xo : inflate.h

fileio.o fileio.xo : ebcdic.h

funzip.o : crc32.h
crc32.o crc32.fo crc32.xo crypt.o crypt.fo crypt.xo : crc32.h
fileio.o fileio.xo extract.o extract.xo process.o process.xo : crc32.h


crypt.o crypt.fo crypt.xo ttyio.o ttyio.fo ttyio.xo : crypt.h
unzip.o unzip.xo funzip.o : crypt.h
fileio.o fileio.xo extract.o extract.xo : crypt.h
inflate.o inflate.fo inflate.xo : crypt.h

crypt.o crypt.fo crypt.xo ttyio.o ttyio.fo ttyio.xo : ttyio.h
funzip.o fileio.o fileio.xo : ttyio.h

unzip.o unzip.xo : unzvers.h consts.h


# Special case object files:

qdos.o: qdos/qdos.c
	cp qdos/qdos.c .
	$(CC) -c -oqdos.o $(CFLAGS) $*.c
	rm -f qdos.c

config.o: qdos/config.S
	cp qdos/config.S .
	$(CC) -c $*.S
	rm -f config.S

qdos.xo: qdos/qdos.c
	cp qdos/qdos.c .
	qcpp -DSFX -DSFX_EXDIR -I. -I./qdos/ qdos.c qdos.i
	c68 qdos.i qdos.s
	as68 qdos.s qdos.xo
	rm -f qdos.s qdos.i
	rm -f qdos.c

qdos.fo: qdos/qdos.c
	cp qdos/qdos.c .
	qcpp -DFUNZIP -I. -I./qdos/ qdos.c qdos.i
	c68 qdos.i qdos.s
	as68 qdos.s qdos.fo
	rm -f qdos.s qdos.i
	rm -f qdos.c

install:  unzip unzipsfx funzip makesfx callstub
	qcp unzip /ql/exe/
	qcp unzipsfx /ql/exe/
	qcp funzip /ql/exe/
	qcp makesfx /ql/exe/
	cp callstub /ql/exe/
