#------------------------------------------------------------------------------
# Makefile for UnZip 5.53 and later                            Derek Fawcus
# Version:  MetaWare High C with PAMAKE                        30 Dec 2005

#
###########################################################################
#
#  Alter the first two macros to change the model
#

MODEL		= big
M		= b

#
###########################################################################
#
#  These two sets of prefixes are changed when swapping between the master
#  and my machine.
#

FL_PREFIX	= d:/flexos		# Where FlexOS is rooted
#FL_PREFIX	= c:/.

#
###########################################################################
#
#  The following should not have to be changed - they should be automatic
#  given correct values for the above.
#

HC_LIB		= $(FL_PREFIX)/highc/$(MODEL)/hc$(M)e.l86
PORT_LIB	= $(FL_PREFIX)/usr/portlib/$(MODEL)/prtlbhc$(M).l86
PORT_OBJ	= $(FL_PREFIX)/usr/portlib/$(MODEL)/prtlbhc$(M).obj

HC_INC		= $(FL_PREFIX)/highc/inc
PORT_INC	= $(FL_PREFIX)/usr/portlib/inc
FLEX_INC	= $(FL_PREFIX)/usr/inc

+IPATH		= $(PORT_INC)/;$(FLEX_INC)/;
#+IPATH		= $(PORT_INC)/;$(HC_INC)/;$(FLEX_INC)/;

#
###########################################################################
#
#  The programs to do the work.
#

CC		= hc
AS		= rasm86
LD		= link86

#HCOPTS=-mm $(MODEL) -debug -pr flexos/hc.pro
HCOPTS=-mm $(MODEL) -pr flexos/hc.pro

#
###########################################################################
#
#  The file extensions to build from
#

.SUFFIXES:
.SUFFIXES: .c .h .a86

#
###########################################################################
#

# Type for CPU required: 0: 8086, 1: 80186, 2: 80286, 3: 80386, etc.
CPU_TYP = 2

# Uncomment the following three macros to use the optimized CRC32 assembler
# routine in UnZip and UnZipSFX:
#ASMFLG = -DASM_CRC
#ASMOBJS = crc_i86.obj
#ASMOBJF = crc_i86_.obj

ASCPUFLAG = __$(CPU_TYP)86

UNZIP_H = unzip.h unzpriv.h globals.h flexos/flxcfg.h

default:        unzip.286
#default:        unzip.286 funzip.286 unzipsfx.286

clean:
	del *.ob?
	del unzip.286
	del funzip.286
	del unzipsfx.*
	del *.map
	del *.lin
	del *.sym
	del *.dbg

#
###########################################################################
#

.asm.obj:
	$(AS) $(ASFLAGS) -D$(ASUNMODEL) $*.asm

.c.obj:
	$(CC) $< $(HCOPTS) -ob $@

.c.obf:
	$(CC) $< $(HCOPTS) -def FUNZIP -ob $@

.c.obs:
	$(CC) $< $(HCOPTS) -def SFX -ob $@

#
###########################################################################
#

crc_i86.obj: flexos/crc_i86.a86
	$(AS) $(ASFLAGS) -D$(ASUNMODEL) flexos/crc_i86.a86, $*.obj ;

#
#########################################################################
#
#  The objects to build from
#

UNZIP_OBJS =	unzip.obj crc32.obj crypt.obj envargs.obj \
		explode.obj extract.obj fileio.obj globals.obj inflate.obj \
		list.obj match.obj process.obj ttyio.obj unreduce.obj \
		unshrink.obj zipinfo.obj flexos.obj $(ASMOBJS)

unzip.286: $(UNZIP_OBJS)
	$(LD) $@[st[add[17e]]] = $-[input]
$(PORT_OBJ),
$+(${UNZIP_OBJS}),
$(PORT_LIB),
$(HC_LIB)
<

UNZIPSFX_OBJS =	unzip.obs crc32.obs crypt.obs extract.obs \
		fileio.obs globals.obs inflate.obs match.obs process.obs \
		ttyio.obs flexos.obs $(ASMOBJS)

unzipsfx.286: $(UNZIPSFX_OBJS)
	$(LD) $@[dbi,map[all],st[add[17e]]] = $-[input]
$(PORT_OBJ),
$+(${UNZIPSFX_OBJS}),
$(PORT_LIB),
$(HC_LIB)
<

FUNZIP_OBJS =	funzip.obj crc32.obf crypt.obf globals.obf inflate.obf \
		ttyio.obf $(ASMOBJF)

funzip.286: $(FUNZIP_OBJS)
	$(LD) $@[dbi,map[all],st[add[17e]]] = $-[input]
$(PORT_OBJ),
$+(${FUNZIP_OBJS}),
$(PORT_LIB),
$(HC_LIB)
<

#
###########################################################################
#

unreduce.obj:	unreduce.c $(UNZIP_H)
unshrink.obj:	unshrink.c $(UNZIP_H)
unzip.obj:	unzip.c $(UNZIP_H) crypt.h unzvers.h consts.h
zipinfo.obj:	zipinfo.c $(UNZIP_H)
process.obj:	process.c $(UNZIP_H) crc32.h
list.obj:	list.c $(UNZIP_H)
match.obj:	match.c $(UNZIP_H)
fileio.obj:	fileio.c $(UNZIP_H) crc32.h crypt.h ttyio.h ebcdic.h
envargs.obj:	envargs.c $(UNZIP_H)
explode.obj:	explode.c $(UNZIP_H)
extract.obj:	extract.c $(UNZIP_H) crc32.h crypt.h
crc32.obj:	crc32.c $(UNZIP_H) zip.h crc32.h

flexos.obj:	flexos/flexos.c $(UNZIP_H)
crc32.obj:	crc32.c $(UNZIP_H) zip.h crc32.h
crypt.obj:	crypt.c $(UNZIP_H) crc32.h crypt.h ttyio.h zip.h
globals.obj:	globals.c $(UNZIP_H)
inflate.obj:	inflate.c inflate.h $(UNZIP_H)
ttyio.obj:	ttyio.c $(UNZIP_H) crypt.h ttyio.h zip.h

funzip.obj:	funzip.c $(UNZIP_H) crc32.h crypt.h ttyio.h
crc32.obf:	crc32.c $(UNZIP_H) zip.h crc32.h
crypt.obf:	crypt.c $(UNZIP_H) crypt.h ttyio.h zip.h
globals.obf:	globals.c $(UNZIP_H)
inflate.obf:	inflate.c inflate.h $(UNZIP_H) crypt.h
ttyio.obf:	ttyio.c $(UNZIP_H) crypt.h ttyio.h zip.h

flexos.obs:	flexos/flexos.c $(UNZIP_H)
crc32.obs:	crc32.c $(UNZIP_H) zip.h crc32.h
extract.obs:	extract.c $(UNZIP_H) crc32.h crypt.h
fileio.obs:	fileio.c $(UNZIP_H) crc32.h crypt.h ttyio.h ebcdic.h
globals.obs:	globals.c $(UNZIP_H)
inflate.obs:	inflate.c inflate.h $(UNZIP_H) crypt.h
match.obs:	match.c $(UNZIP_H)
process.obs:	process.c $(UNZIP_H) crc32.h
ttyio.obs:	ttyio.c $(UNZIP_H) crypt.h ttyio.h zip.h
unzip.obs:	unzip.c $(UNZIP_H) crypt.h unzvers.h consts.h
