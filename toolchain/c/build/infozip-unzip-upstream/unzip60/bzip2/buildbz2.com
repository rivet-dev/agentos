$! BUILDBZ2.COM
$!
$!     Build procedure for LIBBZ2_NS support library used with the
$!     VMS versions of UnZip/ZipInfo and UnZipSFX
$!
$!     Last revised:  2007-12-29  CS.
$!
$!     Command args:
$!     - select compiler environment: "VAXC", "DECC", "GNUC"
$!     - select compiler listings: "LIST"  Note that the whole argument
$!       is added to the compiler command, so more elaborate options
$!       like "LIST/SHOW=ALL" (quoted or space-free) may be specified.
$!     - supply additional compiler options: "CCOPTS=xxx"  Allows the
$!       user to add compiler command options like /ARCHITECTURE or
$!       /[NO]OPTIMIZE.  For example, CCOPTS=/ARCH=HOST/OPTI=TUNE=HOST
$!       or CCOPTS=/DEBUG/NOOPTI.  These options must be quoted or
$!       space-free.
$!
$!     To specify additional options, define the symbol LOCAL_BZIP2
$!     as a comma-separated list of the C macros to be defined, and
$!     then run BUILDBZ2.COM.  For example:
$!
$!             $ LOCAL_BZIP2 = "RETURN_CODES"
$!             $ @ []BUILDBZ2.COM
$!
$!     If you edit this procedure to set LOCAL_BZIP2 here, be sure to
$!     use only one "=", to avoid affecting other procedures.
$!
$!
$ on error then goto error
$ on control_y then goto error
$ OLD_VERIFY = f$verify(0)
$!
$ edit := edit                  ! override customized edit commands
$ say := write sys$output
$!
$!##################### Read settings from environment ########################
$!
$ if (f$type(LOCAL_BZIP2) .eqs. "")
$ then
$     local_bzip2 = ""
$ else  ! Trim blanks and append comma if missing
$     local_bzip2 = f$edit(local_bzip2, "TRIM")
$     if (f$extract((f$length(local_bzip2) - 1), 1, local_bzip2) .nes. ",")
$     then
$         local_bzip2 = local_bzip2 + ", "
$     endif
$ endif
$!
$!##################### Customizing section #############################
$!
$ unzx_unx = "UNZIP"
$ unzx_cli = "UNZIP_CLI"
$ unzsfx_unx = "UNZIPSFX"
$ unzsfx_cli = "UNZIPSFX_CLI"
$!
$ CCOPTS = ""
$ LINKOPTS = "/notraceback"
$ LISTING = " /nolist"
$ MAY_USE_DECC = 1
$ MAY_USE_GNUC = 0
$!
$! Process command line parameters requesting optional features.
$!
$ arg_cnt = 1
$ argloop:
$     current_arg_name = "P''arg_cnt'"
$     curr_arg = f$edit( 'current_arg_name', "UPCASE")
$     if (curr_arg .eqs. "") then goto argloop_out
$!
$     if (f$extract( 0, 5, curr_arg) .eqs. "CCOPT")
$     then
$         opts = f$edit( curr_arg, "COLLAPSE")
$         eq = f$locate( "=", opts)
$         CCOPTS = f$extract( (eq+ 1), 1000, opts)
$         goto argloop_end
$     endif
$!
$     if (f$extract( 0, 4, curr_arg) .eqs. "LIST")
$     then
$         LISTING = "/''curr_arg'"      ! But see below for mods.
$         goto argloop_end
$     endif
$!
$     if (curr_arg .eqs. "VAXC")
$     then
$         MAY_USE_DECC = 0
$         MAY_USE_GNUC = 0
$         goto argloop_end
$     endif
$!
$     if (curr_arg .eqs. "DECC")
$     then
$         MAY_USE_DECC = 1
$         MAY_USE_GNUC = 0
$         goto argloop_end
$     endif
$!
$     if (curr_arg .eqs. "GNUC")
$     then
$         MAY_USE_DECC = 0
$         MAY_USE_GNUC = 1
$         goto argloop_end
$     endif
$!
$     say "Unrecognized command-line option: ''curr_arg'"
$     goto error
$!
$     argloop_end:
$     arg_cnt = arg_cnt + 1
$ goto argloop
$ argloop_out:
$!
$!#######################################################################
$!
$! Find out current disk, directory, compiler and options
$!
$ workdir = f$environment("default")
$ here = f$parse(workdir, , , "device") + f$parse(workdir, , , "directory")
$!
$! Sense the host architecture (Alpha, Itanium, or VAX).
$!
$ if (f$getsyi("HW_MODEL") .lt. 1024)
$ then
$     arch = "VAX"
$ else
$     if (f$getsyi("ARCH_TYPE") .eq. 2)
$     then
$         arch = "ALPHA"
$     else
$         if (f$getsyi("ARCH_TYPE") .eq. 3)
$         then
$             arch = "IA64"
$         else
$             arch = "unknown_arch"
$         endif
$     endif
$ endif
$!
$ dest = arch
$ cmpl = "DEC/Compaq/HP C"
$ opts = ""
$ if (arch .nes. "VAX")
$ then
$     HAVE_DECC_VAX = 0
$     USE_DECC_VAX = 0
$!
$     if (MAY_USE_GNUC)
$     then
$         say "GNU C is not supported for ''arch'."
$         say "You must use DEC/Compaq/HP C to build UnZip."
$         goto error
$     endif
$!
$     if (.not. MAY_USE_DECC)
$     then
$         say "VAX C is not supported for ''arch'."
$         say "You must use DEC/Compaq/HP C to build UnZip."
$         goto error
$     endif
$!
$     cc = "cc /standard=relax /prefix=all /ansi /names=(as_is)"
$     defs = "''local_bzip2'"
$ else
$     HAVE_DECC_VAX = (f$search("SYS$SYSTEM:DECC$COMPILER.EXE") .nes. "")
$     HAVE_VAXC_VAX = (f$search("SYS$SYSTEM:VAXC.EXE") .nes. "")
$     MAY_HAVE_GNUC = (f$trnlnm("GNU_CC") .nes. "")
$     if (HAVE_DECC_VAX .and. MAY_USE_DECC)
$     then
$         ! We use DECC:
$         USE_DECC_VAX = 1
$         cc = "cc /decc /prefix=all /names=(as_is)"
$         defs = "''local_bzip2'"
$     else
$         ! We use VAXC (or GNU C):
$         USE_DECC_VAX = 0
$         defs = "''local_bzip2'"
$         if ((.not. HAVE_VAXC_VAX .and. MAY_HAVE_GNUC) .or. MAY_USE_GNUC)
$         then
$             cc = "gcc"
$             dest = "''dest'G"
$             cmpl = "GNU C"
$             opts = "GNU_CC:[000000]GCCLIB.OLB /LIBRARY,"
$         else
$             if (HAVE_DECC_VAX)
$             then
$                 cc = "cc /vaxc"
$             else
$                 cc = "cc"
$             endif
$             dest = "''dest'V"
$             cmpl = "VAX C"
$         endif
$     endif
$ endif
$!
$! Reveal the plan.  If compiling, set some compiler options.
$!
$     say "Compiling bzip2 on ''arch' using ''cmpl'."
$!
$     DEF_NS = "/define = (''defs'BZ_NO_STDIO, VMS)"
$!
$! If [.'dest'] does not exist, either complain (link-only) or make it.
$!
$ if (f$search("''dest'.dir;1") .eqs. "")
$ then
$     create /directory [.'dest']
$ endif
$!
$! Arrange to get arch-specific list file placement, if listing, and if
$! the user didn't specify a particular "/LIST =" destination.
$!
$     L = f$edit(LISTING, "COLLAPSE")
$     if ((f$extract(0, 5, L) .eqs. "/LIST") .and. -
       (f$extract(4, 1, L) .nes. "="))
$     then
$         LISTING = " /LIST = [.''dest']" + f$extract(5, 1000, LISTING)
$     endif
$!
$! Define compiler command.
$!
$     cc = cc + " /include = ([])" + LISTING + CCOPTS
$!
$! Show interesting facts.
$!
$ say "   architecture = ''arch' (destination = [.''dest'])"
$ say "   cc = ''cc'"
$ say ""
$!
$ tmp = f$verify( 1)    ! Turn echo on to see what's happening.
$!
$!------------------------------- BZip2 section ------------------------------
$!
$! Compile the sources.
$!
$     cc 'DEF_NS' /object = [.'dest']blocksort.OBJ blocksort.c
$     cc 'DEF_NS' /object = [.'dest']huffman.OBJ huffman.c
$     cc 'DEF_NS' /object = [.'dest']crctable.OBJ crctable.c
$     cc 'DEF_NS' /object = [.'dest']randtable.OBJ randtable.c
$     cc 'DEF_NS' /object = [.'dest']compress.OBJ compress.c
$     cc 'DEF_NS' /object = [.'dest']decompress.OBJ decompress.c
$     cc 'DEF_NS' /object = [.'dest']bzlib.OBJ bzlib.c
$!
$! Create the object library.
$!
$     if (f$search( "[.''dest']LIBBZ2_NS.OLB") .eqs. "") then -
       libr /object /create [.'dest']LIBBZ2_NS.OLB
$!
$     libr /object /replace [.'dest']LIBBZ2_NS.OLB -
       [.'dest']blocksort.OBJ, -
       [.'dest']huffman.OBJ, -
       [.'dest']crctable.OBJ, -
       [.'dest']randtable.OBJ, -
       [.'dest']compress.OBJ, -
       [.'dest']decompress.OBJ, -
       [.'dest']bzlib.OBJ
$!
$! Restore the original default directory, deassign the temporary
$! logical names, and restore the DCL verify status.
$!
$ error:
$!
$ if (f$type(here) .nes. "")
$ then
$     if (here .nes. "")
$     then
$         set default 'here'
$     endif
$ endif
$!
$ if (f$type(OLD_VERIFY) .nes. "")
$ then
$     tmp = f$verify(OLD_VERIFY)
$ endif
$!
$ exit
$!
