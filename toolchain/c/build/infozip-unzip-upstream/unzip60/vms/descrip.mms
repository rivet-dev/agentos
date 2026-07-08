#                                               1 March 2009.  SMS.
#
#    UnZip 6.0 for VMS - MMS (or MMK) Description File.
#
# Usage:
#
#    MMS /DESCRIP = [.VMS]DESCRIP.MMS [/MACRO = (<see_below>)] [target]
#
# Note that this description file must be used from the main
# distribution directory, not from the [.VMS] subdirectory.
#
# Optional macros:
#
#    USEBZ2=1       Build with optional BZIP2 support.  This macro
#                   is a shortcut for IZ_BZIP2=SYS$DISK:[.BZIP2].
#                   Additionally, it forces invokation of the UnZip-supplied
#                   bzip2 make script provided in [.bzip2]descrbz2.mms.
#                   This results in a "single-command" build of UnZip with
#                   bzip2 support directly from the sources.
#
#    IZ_BZIP2=dev:[dir]  Build with optional BZIP2 support.  The value
#                        of the MMS macro, ("dev:[dir]", or a suitable
#                   logical name) tells where to find "bzlib.h".  The
#                   BZIP2 object library (LIBBZ2_NS.OLB) is expected to
#                   be in a "[.dest]" directory under that one
#                   ("dev:[dir.ALPHAL]", for example), or in that
#                   directory itself.
#                   By default, the SFX programs are built without BZIP2
#                   support.  Add "BZIP2_SFX=1" to the LOCAL_UNZIP C
#                   macros to enable it.  (See LOCAL_UNZIP, below.)
#
#    IZ_ZLIB=dev:[dir]  Use ZLIB compression library instead of internal
#                       compression routines.  The value of the MMS
#                   macro ("dev:[dir]", or a suitable logical name)
#                   tells where to find "zlib.h".  The ZLIB object
#                   library (LIBZ.OLB) is expected to be in a
#                   "[.dest]" directory under that one
#                   ("dev:[dir.ALPHAL]", for example), or in that
#                   directory itself.
#
#    CCOPTS=xxx     Compile with CC options xxx.  For example:
#                   CCOPTS=/ARCH=HOST
#
#    DBG=1          Compile with /DEBUG /NOOPTIMIZE.
#                   Link with /DEBUG /TRACEBACK.
#                   (Default is /NOTRACEBACK.)
#
#    LARGE=1        Enable large-file (>2GB) support.  Non-VAX only.
#
#    LINKOPTS=xxx   Link with LINK options xxx.  For example:
#                   LINKOPTS=/NOINFO
#
#    LIST=1         Compile with /LIST /SHOW = (ALL, NOMESSAGES).
#                   Link with /MAP /CROSS_REFERENCE /FULL.
#
#    NOSHARE=1      Link /NOSYSSHR (not using shareable images).
#    NOSHARE=OLDVAX Link /NOSYSSHR on VAX for:
#                      DEC C with VMS before V7.3.
#                      VAX C without DEC C RTL (DEC C not installed).
#
#    "LOCAL_UNZIP= c_macro_1=value1 [, c_macro_2=value2 [...]]"
#                   Compile with these additional C macros defined.
#
# VAX-specific optional macros:
#
#    VAXC=1         Use the VAX C compiler, assuming "CC" runs it.
#                   (That is, DEC C is not installed, or else DEC C is
#                   installed, but VAX C is the default.)
#
#    FORCE_VAXC=1   Use the VAX C compiler, assuming "CC /VAXC" runs it.
#                   (That is, DEC C is installed, and it is the
#                   default, but you want VAX C anyway, you fool.)
#
#    GNUC=1         Use the GNU C compiler.  (Seriously under-tested.)
#
#
# The default target, ALL, builds the selected product executables and
# help files.
#
# Other targets:
#
#    CLEAN      deletes architecture-specific files, but leaves any
#               individual source dependency files and the help files.
#
#    CLEAN_ALL  deletes all generated files, except the main (collected)
#               source dependency file.
#
#    CLEAN_EXE  deletes only the architecture-specific executables.
#               Handy if all you wish to do is re-link the executables.
#
# Example commands:
#
# To build the conventional small-file product using the DEC/Compaq/HP C
# compiler (Note: DESCRIP.MMS is the default description file name.):
#
#    MMS /DESCRIP = [.VMS]
#
# To get the large-file executables (on a non-VAX system):
#
#    MMS /DESCRIP = [.VMS] /MACRO = (LARGE=1)
#
# To delete the architecture-specific generated files for this system
# type:
#
#    MMS /DESCRIP = [.VMS] /MACRO = (LARGE=1) CLEAN     ! Large-file.
# or
#    MMS /DESCRIP = [.VMS] CLEAN                        ! Small-file.
#
# To build a complete small-file product for debug with compiler
# listings and link maps:
#
#    MMS /DESCRIP = [.VMS] CLEAN
#    MMS /DESCRIP = [.VMS] /MACRO = (DBG=1, LIST=1)
#
########################################################################

# Include primary product description file.

INCL_DESCRIP_SRC = 1
.INCLUDE [.VMS]DESCRIP_SRC.MMS

# Object library names.

LIB_UNZIP = SYS$DISK:[.$(DEST)]UNZIP.OLB
LIB_UNZIP_CLI = SYS$DISK:[.$(DEST)]UNZIPCLI.OLB
LIB_UNZIPSFX = SYS$DISK:[.$(DEST)]UNZIPSFX.OLB
LIB_UNZIPSFX_CLI = SYS$DISK:[.$(DEST)]UNZSFXCLI.OLB

# Help file names.

UNZIP_HELP = UNZIP.HLP UNZIP_CLI.HLP

# Message file names.

UNZIP_MSG_MSG = [.VMS]UNZIP_MSG.MSG
UNZIP_MSG_EXE = [.$(DEST)]UNZIP_MSG.EXE
UNZIP_MSG_OBJ = [.$(DEST)]UNZIP_MSG.OBJ


# TARGETS.

# Default target, ALL.  Build All executables,
# and help files.

ALL : $(UNZIP) $(UNZIP_CLI) $(UNZIPSFX) $(UNZIPSFX_CLI) $(UNZIP_HELP) \
      $(UNZIP_MSG_EXE)
	@ write sys$output "Done."

# CLEAN target.  Delete the [.$(DEST)] directory and everything in it.

CLEAN :
	if (f$search( "[.$(DEST)]*.*") .nes. "") then -
	 delete [.$(DEST)]*.*;*
	if (f$search( "$(DEST).dir") .nes. "") then -
	 set protection = w:d $(DEST).dir;*
	if (f$search( "$(DEST).dir") .nes. "") then -
	 delete $(DEST).dir;*

# CLEAN_ALL target.  Delete:
#    The [.$(DEST)] directories and everything in them.
#    All help-related derived files,
#    All individual C dependency files.
# Also mention:
#    Comprehensive dependency file.
#
CLEAN_ALL :
	if (f$search( "[.ALPHA*]*.*") .nes. "") then -
	 delete [.ALPHA*]*.*;*
	if (f$search( "ALPHA*.dir", 1) .nes. "") then -
	 set protection = w:d ALPHA*.dir;*
	if (f$search( "ALPHA*.dir", 2) .nes. "") then -
	 delete ALPHA*.dir;*
	if (f$search( "[.IA64*]*.*") .nes. "") then -
	 delete [.IA64*]*.*;*
	if (f$search( "IA64*.dir", 1) .nes. "") then -
	 set protection = w:d IA64*.dir;*
	if (f$search( "IA64*.dir", 2) .nes. "") then -
	 delete IA64*.dir;*
	if (f$search( "[.VAX*]*.*") .nes. "") then -
	 delete [.VAX*]*.*;*
	if (f$search( "VAX*.dir", 1) .nes. "") then -
	 set protection = w:d VAX*.dir;*
	if (f$search( "VAX*.dir", 2) .nes. "") then -
	 delete VAX*.dir;*
	if (f$search( "[.VMS]UNZIP_CLI.RNH") .nes. "") then -
	 delete [.VMS]UNZIP_CLI.RNH;*
	if (f$search( "UNZIP_CLI.HLP") .nes. "") then -
	 delete UNZIP_CLI.HLP;*
	if (f$search( "UNZIP.HLP") .nes. "") then -
	 delete UNZIP.HLP;*
	if (f$search( "*.MMSD") .nes. "") then -
	 delete *.MMSD;*
	if (f$search( "[.VMS]*.MMSD") .nes. "") then -
	 delete [.VMS]*.MMSD;*
	@ write sys$output ""
	@ write sys$output "Note:  This procedure will not"
	@ write sys$output "   DELETE [.VMS]DESCRIP_DEPS.MMS;*"
	@ write sys$output -
 "You may choose to, but a recent version of MMS (V3.5 or newer?) is"
	@ write sys$output -
 "needed to regenerate it.  (It may also be recovered from the original"
	@ write sys$output -
 "distribution kit.)  See [.VMS]DESCRIP_MKDEPS.MMS for instructions on"
	@ write sys$output -
 "generating [.VMS]DESCRIP_DEPS.MMS."
        @ write sys$output ""

# CLEAN_EXE target.  Delete the executables in [.$(DEST)].

CLEAN_EXE :
        if (f$search( "[.$(DEST)]*.EXE") .nes. "") then -
         delete [.$(DEST)]*.EXE;*


# Object library module dependencies.

$(LIB_UNZIP) : $(LIB_UNZIP)($(MODS_OBJS_LIB_UNZIP))
	@ write sys$output "$(MMS$TARGET) updated."

$(LIB_UNZIP_CLI) : $(LIB_UNZIP_CLI)($(MODS_OBJS_LIB_UNZIP_CLI))
	@ write sys$output "$(MMS$TARGET) updated."

$(LIB_UNZIPSFX) : $(LIB_UNZIPSFX)($(MODS_OBJS_LIB_UNZIPSFX))
	@ write sys$output "$(MMS$TARGET) updated."

$(LIB_UNZIPSFX_CLI) : $(LIB_UNZIPSFX_CLI)($(MODS_OBJS_LIB_UNZIPSFX_CLI))
	@ write sys$output "$(MMS$TARGET) updated."


# Module ID options files.

OPT_ID = SYS$DISK:[.VMS]UNZIP.OPT
OPT_ID_SFX = SYS$DISK:[.VMS]UNZIPSFX.OPT

# Default C compile rule.

.C.OBJ :
        $(CC) $(CFLAGS) $(CDEFS_UNX) $(MMS$SOURCE)


# Normal sources in [.VMS].

[.$(DEST)]VMS.OBJ : [.VMS]VMS.C

# Command-line interface files.

[.$(DEST)]CMDLINE.OBJ : [.VMS]CMDLINE.C
	$(CC) $(CFLAGS) $(CDEFS_CLI) $(MMS$SOURCE)

[.$(DEST)]UNZIPCLI.OBJ : UNZIP.C
	$(CC) $(CFLAGS) $(CDEFS_CLI) $(MMS$SOURCE)

[.$(DEST)]UNZ_CLI.OBJ : [.VMS]UNZ_CLI.CLD

# SFX variant sources.

[.$(DEST)]CRC32_.OBJ : CRC32.C
	$(CC) $(CFLAGS) $(CDEFS_SFX) $(MMS$SOURCE)

[.$(DEST)]CRYPT_.OBJ : CRYPT.C
	$(CC) $(CFLAGS) $(CDEFS_SFX) $(MMS$SOURCE)

[.$(DEST)]EXTRACT_.OBJ : EXTRACT.C
	$(CC) $(CFLAGS) $(CDEFS_SFX) $(MMS$SOURCE)

[.$(DEST)]FILEIO_.OBJ : FILEIO.C
	$(CC) $(CFLAGS) $(CDEFS_SFX) $(MMS$SOURCE)

[.$(DEST)]GLOBALS_.OBJ : GLOBALS.C
	$(CC) $(CFLAGS) $(CDEFS_SFX) $(MMS$SOURCE)

[.$(DEST)]INFLATE_.OBJ : INFLATE.C
	$(CC) $(CFLAGS) $(CDEFS_SFX) $(MMS$SOURCE)

[.$(DEST)]MATCH_.OBJ : MATCH.C
	$(CC) $(CFLAGS) $(CDEFS_SFX) $(MMS$SOURCE)

[.$(DEST)]PROCESS_.OBJ : PROCESS.C
	$(CC) $(CFLAGS) $(CDEFS_SFX) $(MMS$SOURCE)

[.$(DEST)]TTYIO_.OBJ : TTYIO.C
	$(CC) $(CFLAGS) $(CDEFS_SFX) $(MMS$SOURCE)

[.$(DEST)]UBZ2ERR_.OBJ : UBZ2ERR.C
	$(CC) $(CFLAGS) $(CDEFS_SFX) $(MMS$SOURCE)

[.$(DEST)]VMS_.OBJ : [.VMS]VMS.C
	$(CC) $(CFLAGS) $(CDEFS_SFX) $(MMS$SOURCE)

[.$(DEST)]UNZIPSFX.OBJ : UNZIP.C
	$(CC) $(CFLAGS) $(CDEFS_SFX) $(MMS$SOURCE)

# SFX CLI variant sources.

[.$(DEST)]CMDLINE_.OBJ : [.VMS]CMDLINE.C
	$(CC) $(CFLAGS) $(CDEFS_SFX_CLI) $(MMS$SOURCE)

[.$(DEST)]UNZSFXCLI.OBJ : UNZIP.C
	$(CC) $(CFLAGS) $(CDEFS_SFX_CLI) $(MMS$SOURCE)

# VAX C LINK options file.

.IFDEF OPT_FILE
$(OPT_FILE) :
	open /write opt_file_ln  $(OPT_FILE)
	write opt_file_ln "SYS$SHARE:VAXCRTL.EXE /SHARE"
	close opt_file_ln
.ENDIF

# Local BZIP2 object library.

$(LIB_BZ2_LOCAL) :
	$(MMS) $(MMSQUALIFIERS) /DESCR=$(IZ_BZIP2)descrbz2.mms'macro' -
	   /MACRO = (SRCDIR=$(IZ_BZIP2), DSTDIR=$(BZ2DIR_BIN), -
	   DEST=$(IZ_BZIP2)$(DESTM)) $(MMSTARGETS)

# Normal UnZip executable.

$(UNZIP) : [.$(DEST)]UNZIP.OBJ \
           $(LIB_UNZIP) $(LIB_BZ2_DEP) $(OPT_FILE) $(OPT_ID)
	$(LINK) $(LINKFLAGS) $(MMS$SOURCE), -
	 $(LIB_UNZIP) /library, -
	 $(LIB_BZIP2_OPTS) -
	 $(LIB_UNZIP) /library, -
	 $(LIB_ZLIB_OPTS) -
	 $(LFLAGS_ARCH) -
	 $(OPT_ID) /options -
	 $(NOSHARE_OPTS)

# CLI UnZip executable.

$(UNZIP_CLI) : [.$(DEST)]UNZIPCLI.OBJ \
               $(LIB_UNZIP_CLI) $(LIB_BZ2_DEP) $(OPT_FILE) $(OPT_ID)
	$(LINK) $(LINKFLAGS) $(MMS$SOURCE), -
	 $(LIB_UNZIP_CLI) /library, -
	 $(LIB_UNZIP) /library, -
	 $(LIB_BZIP2_OPTS) -
	 $(LIB_UNZIP) /library, -
	 $(LIB_ZLIB_OPTS) -
	 $(LFLAGS_ARCH) -
	 $(OPT_ID) /options -
	 $(NOSHARE_OPTS)

# SFX UnZip executable.

$(UNZIPSFX) : [.$(DEST)]UNZIPSFX.OBJ \
              $(LIB_UNZIPSFX) $(LIB_BZ2_DEP) $(OPT_FILE) $(OPT_ID_SFX)
	$(LINK) $(LINKFLAGS) $(MMS$SOURCE), -
	 $(LIB_UNZIPSFX) /library, -
	 $(LIB_BZIP2_OPTS) -
	 $(LIB_UNZIPSFX) /library, -
	 $(LIB_ZLIB_OPTS) -
	 $(LFLAGS_ARCH) -
	 $(OPT_ID_SFX) /options -
	 $(NOSHARE_OPTS)

# SFX CLI UnZip executable.

$(UNZIPSFX_CLI) : [.$(DEST)]UNZSFXCLI.OBJ \
                  $(LIB_UNZIPSFX_CLI) $(LIB_UNZIPSFX) $(LIB_BZ2_DEP) \
                  $(OPT_FILE) $(OPT_ID_SFX)
	$(LINK) $(LINKFLAGS) $(MMS$SOURCE), -
	 $(LIB_UNZIPSFX_CLI) /library, -
	 $(LIB_UNZIPSFX) /library, -
	 $(LIB_BZIP2_OPTS) -
	 $(LIB_UNZIPSFX) /library, -
	 $(LIB_ZLIB_OPTS) -
	 $(LFLAGS_ARCH) -
	 $(OPT_ID_SFX) /options -
	 $(NOSHARE_OPTS)


# Help files.

UNZIP.HLP : [.VMS]UNZIP_DEF.RNH
	runoff /output = $(MMS$TARGET) $(MMS$SOURCE)

UNZIP_CLI.HLP : [.VMS]UNZIP_CLI.HELP [.VMS]CVTHELP.TPU
	edit /tpu /nosection /nodisplay /command = [.VMS]CVTHELP.TPU -
	 $(MMS$SOURCE)
	rename UNZIP_CLI.RNH [.VMS]
        purge /nolog /keep = 1 [.VMS]UNZIP_CLI.RNH
	runoff /output = $(MMS$TARGET) [.VMS]UNZIP_CLI.RNH

# Message file.

$(UNZIP_MSG_EXE) : $(UNZIP_MSG_OBJ)
        link /shareable = $(MMS$TARGET) $(UNZIP_MSG_OBJ)

$(UNZIP_MSG_OBJ) : $(UNZIP_MSG_MSG)
        message /object = $(MMS$TARGET) /nosymbols $(UNZIP_MSG_MSG)

# Include generated source dependencies.

INCL_DESCRIP_DEPS = 1
.INCLUDE [.VMS]DESCRIP_DEPS.MMS

