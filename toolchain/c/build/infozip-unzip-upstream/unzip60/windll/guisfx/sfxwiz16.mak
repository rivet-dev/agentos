#
# Borland C++ IDE generated makefile
#
.AUTODEPEND


#
# Borland C++ tools
#
IMPLIB  = Implib
BCC     = Bcc +BccW16.cfg
TLINK   = TLink
TLIB    = TLib
BRC     = Brc
TASM    = Tasm
#
# IDE macros
#


#
# Options
#
IDE_LFLAGS =  -LE:\BC45\LIB
IDE_RFLAGS =
LLATW16_EcbWIZbUNZIPbWINDLLbGUISFXbBINbsfxwizdexe =  -Twe -c -C -LE:\BC45\LIB;E:\WIZ\UNZIP\WINDLL\GUISFX\BIN -n
RLATW16_EcbWIZbUNZIPbWINDLLbGUISFXbBINbsfxwizdexe =  -31
BLATW16_EcbWIZbUNZIPbWINDLLbGUISFXbBINbsfxwizdexe =
CNIEAT_EcbWIZbUNZIPbWINDLLbGUISFXbBINbsfxwizdexe = -IE:\BC45\INCLUDE;E:\WIZ\UNZIP;E:\WIZ\UNZIP\WINDLL;E:\WIZ\UNZIP\WINDLL\GUISFX -DDLL;API;USE_UNZIP_LIB;
LNIEAT_EcbWIZbUNZIPbWINDLLbGUISFXbBINbsfxwizdexe = -x
LEAT_EcbWIZbUNZIPbWINDLLbGUISFXbBINbsfxwizdexe = $(LLATW16_EcbWIZbUNZIPbWINDLLbGUISFXbBINbsfxwizdexe)
REAT_EcbWIZbUNZIPbWINDLLbGUISFXbBINbsfxwizdexe = $(RLATW16_EcbWIZbUNZIPbWINDLLbGUISFXbBINbsfxwizdexe)
BEAT_EcbWIZbUNZIPbWINDLLbGUISFXbBINbsfxwizdexe = $(BLATW16_EcbWIZbUNZIPbWINDLLbGUISFXbBINbsfxwizdexe)
CLATW16_EcbWIZbUNZIPbWINDLLbGUISFXbBINbunzsfx16dlib =  -ml -WS
LLATW16_EcbWIZbUNZIPbWINDLLbGUISFXbBINbunzsfx16dlib =  -Twe -c -C
RLATW16_EcbWIZbUNZIPbWINDLLbGUISFXbBINbunzsfx16dlib =  -31
BLATW16_EcbWIZbUNZIPbWINDLLbGUISFXbBINbunzsfx16dlib =
CEAT_EcbWIZbUNZIPbWINDLLbGUISFXbBINbunzsfx16dlib = $(CEAT_EcbWIZbUNZIPbWINDLLbGUISFXbBINbsfxwizdexe) $(CLATW16_EcbWIZbUNZIPbWINDLLbGUISFXbBINbunzsfx16dlib)
CNIEAT_EcbWIZbUNZIPbWINDLLbGUISFXbBINbunzsfx16dlib = -IE:\BC45\INCLUDE;E:\WIZ\UNZIP;E:\WIZ\UNZIP\WINDLL;E:\WIZ\UNZIP\WINDLL\GUISFX -DSFX;DLL;USE_EF_UT_TIME;UNZIPLIB;WINDLL
LNIEAT_EcbWIZbUNZIPbWINDLLbGUISFXbBINbunzsfx16dlib = -x
LEAT_EcbWIZbUNZIPbWINDLLbGUISFXbBINbunzsfx16dlib = $(LEAT_EcbWIZbUNZIPbWINDLLbGUISFXbBINbsfxwizdexe) $(LLATW16_EcbWIZbUNZIPbWINDLLbGUISFXbBINbunzsfx16dlib)
REAT_EcbWIZbUNZIPbWINDLLbGUISFXbBINbunzsfx16dlib = $(REAT_EcbWIZbUNZIPbWINDLLbGUISFXbBINbsfxwizdexe) $(RLATW16_EcbWIZbUNZIPbWINDLLbGUISFXbBINbunzsfx16dlib)
BEAT_EcbWIZbUNZIPbWINDLLbGUISFXbBINbunzsfx16dlib = $(BEAT_EcbWIZbUNZIPbWINDLLbGUISFXbBINbsfxwizdexe) $(BLATW16_EcbWIZbUNZIPbWINDLLbGUISFXbBINbunzsfx16dlib)

#
# Dependency List
#
Dep_sfxwiz = \
   E:\WIZ\UNZIP\WINDLL\GUISFX\BIN\sfxwiz.exe

sfxwiz : BccW16.cfg $(Dep_sfxwiz)
  echo MakeNode

Dep_EcbWIZbUNZIPbWINDLLbGUISFXbBINbsfxwizdexe = \
   E:\WIZ\UNZIP\WINDLL\GUISFX\BIN\unzsfx16.lib\
   E:\WIZ\UNZIP\WINDLL\GUISFX\OBJ\sfxwiz.res\
   E:\WIZ\UNZIP\WINDLL\GUISFX\OBJ\sfxwiz.obj

E:\WIZ\UNZIP\WINDLL\GUISFX\BIN\sfxwiz.exe : $(Dep_EcbWIZbUNZIPbWINDLLbGUISFXbBINbsfxwizdexe)
  $(TLINK)   @&&|
 /v $(IDE_LFLAGS) $(LEAT_EcbWIZbUNZIPbWINDLLbGUISFXbBINbsfxwizdexe) $(LNIEAT_EcbWIZbUNZIPbWINDLLbGUISFXbBINbsfxwizdexe) +
E:\BC45\LIB\c0wl.obj+
E:\WIZ\UNZIP\WINDLL\GUISFX\OBJ\sfxwiz.obj
$<,$*
E:\WIZ\UNZIP\WINDLL\GUISFX\BIN\unzsfx16.lib+
E:\BC45\LIB\import.lib+
E:\BC45\LIB\mathwl.lib+
E:\BC45\LIB\cwl.lib

|
   $(BRC) E:\WIZ\UNZIP\WINDLL\GUISFX\OBJ\sfxwiz.res $<

Dep_EcbWIZbUNZIPbWINDLLbGUISFXbBINbunzsfx16dlib = \
   unzsfx16.def\
   E:\WIZ\UNZIP\WINDLL\GUISFX\OBJ\crypt.obj\
   E:\WIZ\UNZIP\WINDLL\GUISFX\OBJ\explode.obj\
   E:\WIZ\UNZIP\WINDLL\GUISFX\OBJ\extract.obj\
   E:\WIZ\UNZIP\WINDLL\GUISFX\OBJ\fileio.obj\
   E:\WIZ\UNZIP\WINDLL\GUISFX\OBJ\globals.obj\
   E:\WIZ\UNZIP\WINDLL\GUISFX\OBJ\inflate.obj\
   E:\WIZ\UNZIP\WINDLL\GUISFX\OBJ\match.obj\
   E:\WIZ\UNZIP\WINDLL\GUISFX\OBJ\process.obj\
   E:\WIZ\UNZIP\WINDLL\GUISFX\OBJ\crc32.obj\
   E:\WIZ\UNZIP\WINDLL\GUISFX\OBJ\api.obj\
   E:\WIZ\UNZIP\WINDLL\GUISFX\OBJ\msdos.obj\
   E:\WIZ\UNZIP\WINDLL\GUISFX\OBJ\windll.obj

E:\WIZ\UNZIP\WINDLL\GUISFX\BIN\unzsfx16.lib : $(Dep_EcbWIZbUNZIPbWINDLLbGUISFXbBINbunzsfx16dlib)
  $(TLIB) $< $(IDE_BFLAGS) $(BEAT_EcbWIZbUNZIPbWINDLLbGUISFXbBINbunzsfx16dlib) @&&|
 -+E:\WIZ\UNZIP\WINDLL\GUISFX\OBJ\crypt.obj &
-+E:\WIZ\UNZIP\WINDLL\GUISFX\OBJ\explode.obj &
-+E:\WIZ\UNZIP\WINDLL\GUISFX\OBJ\extract.obj &
-+E:\WIZ\UNZIP\WINDLL\GUISFX\OBJ\fileio.obj &
-+E:\WIZ\UNZIP\WINDLL\GUISFX\OBJ\globals.obj &
-+E:\WIZ\UNZIP\WINDLL\GUISFX\OBJ\inflate.obj &
-+E:\WIZ\UNZIP\WINDLL\GUISFX\OBJ\match.obj &
-+E:\WIZ\UNZIP\WINDLL\GUISFX\OBJ\process.obj &
-+E:\WIZ\UNZIP\WINDLL\GUISFX\OBJ\crc32.obj &
-+E:\WIZ\UNZIP\WINDLL\GUISFX\OBJ\api.obj &
-+E:\WIZ\UNZIP\WINDLL\GUISFX\OBJ\msdos.obj &
-+E:\WIZ\UNZIP\WINDLL\GUISFX\OBJ\windll.obj
|

E:\WIZ\UNZIP\WINDLL\GUISFX\OBJ\crypt.obj :  ..\..\crypt.c
  $(BCC)   -P- -c @&&|
 $(CEAT_EcbWIZbUNZIPbWINDLLbGUISFXbBINbunzsfx16dlib) $(CNIEAT_EcbWIZbUNZIPbWINDLLbGUISFXbBINbunzsfx16dlib) -o$@ ..\..\crypt.c
|

E:\WIZ\UNZIP\WINDLL\GUISFX\OBJ\explode.obj :  ..\..\explode.c
  $(BCC)   -P- -c @&&|
 $(CEAT_EcbWIZbUNZIPbWINDLLbGUISFXbBINbunzsfx16dlib) $(CNIEAT_EcbWIZbUNZIPbWINDLLbGUISFXbBINbunzsfx16dlib) -o$@ ..\..\explode.c
|

E:\WIZ\UNZIP\WINDLL\GUISFX\OBJ\extract.obj :  ..\..\extract.c
  $(BCC)   -P- -c @&&|
 $(CEAT_EcbWIZbUNZIPbWINDLLbGUISFXbBINbunzsfx16dlib) $(CNIEAT_EcbWIZbUNZIPbWINDLLbGUISFXbBINbunzsfx16dlib) -o$@ ..\..\extract.c
|

E:\WIZ\UNZIP\WINDLL\GUISFX\OBJ\fileio.obj :  ..\..\fileio.c
  $(BCC)   -P- -c @&&|
 $(CEAT_EcbWIZbUNZIPbWINDLLbGUISFXbBINbunzsfx16dlib) $(CNIEAT_EcbWIZbUNZIPbWINDLLbGUISFXbBINbunzsfx16dlib) -o$@ ..\..\fileio.c
|

E:\WIZ\UNZIP\WINDLL\GUISFX\OBJ\globals.obj :  ..\..\globals.c
  $(BCC)   -P- -c @&&|
 $(CEAT_EcbWIZbUNZIPbWINDLLbGUISFXbBINbunzsfx16dlib) $(CNIEAT_EcbWIZbUNZIPbWINDLLbGUISFXbBINbunzsfx16dlib) -o$@ ..\..\globals.c
|

E:\WIZ\UNZIP\WINDLL\GUISFX\OBJ\inflate.obj :  ..\..\inflate.c
  $(BCC)   -P- -c @&&|
 $(CEAT_EcbWIZbUNZIPbWINDLLbGUISFXbBINbunzsfx16dlib) $(CNIEAT_EcbWIZbUNZIPbWINDLLbGUISFXbBINbunzsfx16dlib) -o$@ ..\..\inflate.c
|

E:\WIZ\UNZIP\WINDLL\GUISFX\OBJ\match.obj :  ..\..\match.c
  $(BCC)   -P- -c @&&|
 $(CEAT_EcbWIZbUNZIPbWINDLLbGUISFXbBINbunzsfx16dlib) $(CNIEAT_EcbWIZbUNZIPbWINDLLbGUISFXbBINbunzsfx16dlib) -o$@ ..\..\match.c
|

E:\WIZ\UNZIP\WINDLL\GUISFX\OBJ\process.obj :  ..\..\process.c
  $(BCC)   -P- -c @&&|
 $(CEAT_EcbWIZbUNZIPbWINDLLbGUISFXbBINbunzsfx16dlib) $(CNIEAT_EcbWIZbUNZIPbWINDLLbGUISFXbBINbunzsfx16dlib) -o$@ ..\..\process.c
|

E:\WIZ\UNZIP\WINDLL\GUISFX\OBJ\crc32.obj :  ..\..\crc32.c
  $(BCC)   -P- -c @&&|
 $(CEAT_EcbWIZbUNZIPbWINDLLbGUISFXbBINbunzsfx16dlib) $(CNIEAT_EcbWIZbUNZIPbWINDLLbGUISFXbBINbunzsfx16dlib) -o$@ ..\..\crc32.c
|

E:\WIZ\UNZIP\WINDLL\GUISFX\OBJ\api.obj :  ..\..\api.c
  $(BCC)   -P- -c @&&|
 $(CEAT_EcbWIZbUNZIPbWINDLLbGUISFXbBINbunzsfx16dlib) $(CNIEAT_EcbWIZbUNZIPbWINDLLbGUISFXbBINbunzsfx16dlib) -o$@ ..\..\api.c
|

E:\WIZ\UNZIP\WINDLL\GUISFX\OBJ\msdos.obj :  ..\..\msdos\msdos.c
  $(BCC)   -P- -c @&&|
 $(CEAT_EcbWIZbUNZIPbWINDLLbGUISFXbBINbunzsfx16dlib) $(CNIEAT_EcbWIZbUNZIPbWINDLLbGUISFXbBINbunzsfx16dlib) -o$@ ..\..\msdos\msdos.c
|

E:\WIZ\UNZIP\WINDLL\GUISFX\OBJ\windll.obj :  ..\windll.c
  $(BCC)   -P- -c @&&|
 $(CEAT_EcbWIZbUNZIPbWINDLLbGUISFXbBINbunzsfx16dlib) $(CNIEAT_EcbWIZbUNZIPbWINDLLbGUISFXbBINbunzsfx16dlib) -o$@ ..\windll.c
|

E:\WIZ\UNZIP\WINDLL\GUISFX\OBJ\sfxwiz.res :  sfxwiz.rc
  $(BRC) $(IDE_RFLAGS) $(REAT_EcbWIZbUNZIPbWINDLLbGUISFXbBINbsfxwizdexe) $(CNIEAT_EcbWIZbUNZIPbWINDLLbGUISFXbBINbsfxwizdexe) -R -FO$@ sfxwiz.rc

E:\WIZ\UNZIP\WINDLL\GUISFX\OBJ\sfxwiz.obj :  sfxwiz.c
  $(BCC)   -P- -c @&&|
 $(CEAT_EcbWIZbUNZIPbWINDLLbGUISFXbBINbsfxwizdexe) $(CNIEAT_EcbWIZbUNZIPbWINDLLbGUISFXbBINbsfxwizdexe) -o$@ sfxwiz.c
|

# Compiler configuration file
BccW16.cfg :
   Copy &&|
-R
-v
-vi
-H
-H=sfxwiz.csm
-H-
-ml
-WS
-f-
-ff-
-d
-v-
-R-
-Z
-O
-Oe
-Ol
-Ob
-OW
-3
-Og
| $@


