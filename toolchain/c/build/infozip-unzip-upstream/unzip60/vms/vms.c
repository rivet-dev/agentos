/*
  Copyright (c) 1990-2009 Info-ZIP.  All rights reserved.

  See the accompanying file LICENSE, version 2009-Jan-02 or later
  (the contents of which are also included in unzip.h) for terms of use.
  If, for some reason, all these files are missing, the Info-ZIP license
  also may be found at:  ftp://ftp.info-zip.org/pub/infozip/license.html
*/
/*---------------------------------------------------------------------------

  vms.c                                        Igor Mandrichenko and others

  This file contains routines to extract VMS file attributes from a zipfile
  extra field and create a file with these attributes.  The code was almost
  entirely written by Igor, with a couple of routines by GRR and lots of
  modifications and fixes by Christian Spieler.

  Contains:  check_format()
             open_outfile()
             find_vms_attrs()
             flush()
             close_outfile()
             defer_dir_attribs()
             set_direc_attribs()
             dos_to_unix_time()         (TIMESTAMP only)
             stamp_file()               (TIMESTAMP only)
             vms_msg_text()
             do_wild()
             mapattr()
             mapname()
             checkdir()
             check_for_newer()
             return_VMS
             screensize()
             screenlinewrap()
             version()

  ---------------------------------------------------------------------------*/

#ifdef VMS                      /* VMS only! */

#define UNZIP_INTERNAL

#include "unzip.h"
#include "crc32.h"
#include "vms.h"
#include "vmsdefs.h"

#ifdef MORE
#  include <ttdef.h>
#endif
#include <unixlib.h>

#include <dvidef.h>
#include <ssdef.h>
#include <stsdef.h>

/* Workaround for broken header files of older DECC distributions
 * that are incompatible with the /NAMES=AS_IS qualifier. */
#define lib$getdvi LIB$GETDVI
#define lib$getsyi LIB$GETSYI
#define lib$sys_getmsg LIB$SYS_GETMSG
#include <lib$routines.h>

#ifndef EEXIST
#  include <errno.h>    /* For mkdir() status codes */
#endif

/* On VAX, define Goofy VAX Type-Cast to obviate /standard = vaxc.
   Otherwise, lame system headers on VAX cause compiler warnings.
   (GNU C may define vax but not __VAX.)
*/
#ifdef vax
#  define __VAX 1
#endif

#ifdef __VAX
#  define GVTC (unsigned int)
#else
#  define GVTC
#endif

/* With GNU C, some FAB bits may be declared only as masks, not as
 * structure bits.
 */
#ifdef __GNUC__
#  define OLD_FABDEF 1
#endif

#define ASYNCH_QIO              /* Use asynchronous PK-style QIO writes */

/* buffer size for a single block write (using RMS or QIO WRITEVBLK),
   must be less than 64k and a multiple of 512 ! */
#define BUFS512 (((OUTBUFSIZ>0xFFFF) ? 0xFFFF : OUTBUFSIZ) & (~511))
/* buffer size for record output (RMS limit for max. record size) */
#define BUFSMAXREC 32767
/* allocation size for RMS and QIO output buffers */
#define BUFSALLOC (BUFS512 * 2 > BUFSMAXREC ? BUFS512 * 2 : BUFSMAXREC)
        /* locbuf size */

/* VMS success or warning status */
#define OK(s)   (((s) & STS$M_SUCCESS) != 0)
#define STRICMP(s1, s2) STRNICMP(s1, s2, 2147483647)

/* Interactive inquiry response codes for replace(). */

#define REPL_NO_EXTRACT   0
#define REPL_NEW_VERSION  1
#define REPL_OVERWRITE    2
#define REPL_ERRLV_WARN   256
#define REPL_TASKMASK     255

/* 2008-09-13 CS.
 * Note: In extract.c, there are similar strings "InvalidResponse" and
 * "AssumeNone" defined.  However, as the UI functionality of the VMS
 * "version-aware" query is slightly different from the generic variant,
 * these strings are kept separate for now to allow independent
 * "fine tuning" without affecting the other variant of the
 * "overwrite or ..." user query.
 */
ZCONST char Far InvalidResponse[] =
  "error:  invalid response [%.1s]\n";
ZCONST char Far AssumeNo[] =
  "\n(EOF or read error, treating as \"[N]o extract (all)\" ...)\n";


#ifdef SET_DIR_ATTRIB
/* Structure for holding directory attribute data for final processing
 * after all files are in place.
 */
typedef struct vmsdirattr {
    struct vmsdirattr *next;            /* link to next in (linked) list */
    char *fn;                           /* file (directory) name */

    /* Non-VMS attributes data */
    ulg mod_dos_datetime;               /* G.lrec.last_mod_dos_datetime */
    unsigned perms;                     /* same as min_info.file_attr */

    unsigned xlen;                      /* G.lrec.extra_field_length */
    char buf[1];                        /* data buffer (extra_field, fn) */
} vmsdirattr;
#define VmsAtt(d)  ((vmsdirattr *)d)    /* typecast shortcut */
#endif /* SET_DIR_ATTRIB */

/*
 *   Local static storage
 */
static struct FAB        fileblk;       /* File Access Block */
static struct XABDAT     dattim;        /* date-time XAB */
static struct XABRDT     rdt;           /* revision date-time XAB */
static struct RAB        rab;           /* Record Access Block */
static struct NAM_STRUCT nam;           /* name block */

static struct FAB *outfab = NULL;
static struct RAB *outrab = NULL;
static struct XABFHC *xabfhc = NULL;    /* file header characteristics */
static struct XABDAT *xabdat = NULL;    /* date-time */
static struct XABRDT *xabrdt = NULL;    /* revision date-time */
static struct XABPRO *xabpro = NULL;    /* protection */
static struct XABKEY *xabkey = NULL;    /* key (indexed) */
static struct XABALL *xaball = NULL;    /* allocation */
static struct XAB *first_xab = NULL, *last_xab = NULL;

static int replace_code_all = -1;       /* All-file response for replace(). */

static uch rfm;

static uch locbuf[BUFSALLOC];           /* Space for 2 buffers of BUFS512 */
static unsigned loccnt = 0;
static uch *locptr;
static char got_eol = 0;

struct bufdsc
{
    struct bufdsc *next;
    uch *buf;
    unsigned bufcnt;
};

static struct bufdsc b1, b2, *curbuf;   /* buffer ring for asynchronous I/O */

static int  _flush_blocks(__GPRO__ uch *rawbuf, unsigned size, int final_flag);
static int  _flush_stream(__GPRO__ uch *rawbuf, unsigned size, int final_flag);
static int  _flush_varlen(__GPRO__ uch *rawbuf, unsigned size, int final_flag);
static int  _flush_qio(__GPRO__ uch *rawbuf, unsigned size, int final_flag);
static int  _close_rms(__GPRO);
static int  _close_qio(__GPRO);
#ifdef ASYNCH_QIO
static int  WriteQIO(__GPRO__ uch *buf, unsigned len);
#endif
static int  WriteBuffer(__GPRO__ uch *buf, unsigned len);
static int  WriteRecord(__GPRO__ uch *rec, unsigned len);

static int  (*_flush_routine)(__GPRO__ uch *rawbuf, unsigned size,
                              int final_flag);
static int  (*_close_routine)(__GPRO);

#ifdef SYMLINKS
static int  _read_link_rms(__GPRO__ int byte_count, char *link_text_buf);
#endif /* SYMLINKS */

static void init_buf_ring(void);
static void set_default_datetime_XABs(__GPRO);
static int  create_default_output(__GPRO);
static int  create_rms_output(__GPRO);
static int  create_qio_output(__GPRO);
static int  replace(__GPRO);
static int  replace_rms_newversion(__GPRO);
static int  replace_rms_overwrite(__GPRO);
static int  find_vms_attrs(__GPRO__ int set_date_time);
static void free_up(void);
#ifdef CHECK_VERSIONS
static int  get_vms_version(char *verbuf, int len);
#endif /* CHECK_VERSIONS */
static unsigned find_eol(ZCONST uch *p, unsigned n, unsigned *l);
#ifdef SET_DIR_ATTRIB
static char *vms_path_fixdown(ZCONST char *dir_spec, char *dir_file);
#endif
#ifdef TIMESTAMP
static time_t mkgmtime(struct tm *tm);
static void uxtime2vmstime(time_t utimeval, long int binval[2]);
#endif /* TIMESTAMP */
static int vms_msg_fetch(int status);
static void vms_msg(__GPRO__ ZCONST char *string, int status);


/*
   2005-02-14 SMS.
   Added some ODS5 support:
      Use longer name structures in NAML, where available.
      Locate special characters mindful of "^" escapes.
*/

/* Hex digit table. */

char hex_digit[16] = {
 '0', '1', '2', '3', '4', '5', '6', '7',
 '8', '9', 'A', 'B', 'C', 'D', 'E', 'F'
};


/* Character property table for converting Zip file names to
   (simpler) ODS2 or (escaped) ODS5 extended file names.

   ODS2 valid characters: 0-9 A-Z a-z $ - _

   ODS5 Invalid characters:
      C0 control codes (0x00 to 0x1F inclusive)
      Asterisk (*)
      Question mark (?)

   ODS5 Invalid characters only in VMS V7.2 (which no one runs, right?):
      Double quotation marks (")
      Backslash (\)
      Colon (:)
      Left angle bracket (<)
      Right angle bracket (>)
      Slash (/)
      Vertical bar (|)

   Characters escaped by "^":
      SP  !  "  #  %  &  '  (  )  +  ,  .  :  ;  =
       @  [  \  ]  ^  `  {  |  }  ~

   Either "^_" or "^ " is accepted as a space.  Period (.) is a special
   case.  Note that un-escaped < and > can also confuse a directory
   spec.

   Characters put out as ^xx:
      7F (DEL)
      80-9F (C1 control characters)
      A0 (nonbreaking space)
      FF (Latin small letter y diaeresis)

   Other cases:
      Unicode: "^Uxxxx", where "xxxx" is four hex digits.

   Property table values:
      Normal ODS2           1
      Lower-case ODS2       2
      Period                4
      Space                 8
      ODS5 simple          16
      ODS5 1-char escape   32
      ODS5 hex-hex escape  64
*/

unsigned char char_prop[256] = {

/* NUL SOH STX ETX EOT ENQ ACK BEL   BS  HT  LF  VT  FF  CR  SO  SI */
    0,  0,  0,  0,  0,  0,  0,  0,   0,  0,  0,  0,  0,  0,  0,  0,

/* DLE DC1 DC2 DC3 DC4 NAK SYN ETB  CAN  EM SUB ESC  FS  GS  RS  US */
    0,  0,  0,  0,  0,  0,  0,  0,   0,  0,  0,  0,  0,  0,  0,  0,

/*  SP  !   "   #   $   %   &   '    (   )   *   +   ,   -   .   /  */
    8, 32, 32, 32, 17, 32, 32, 32,  32, 32,  0, 32, 32, 17,  4,  0,

/*  0   1   2   3   4   5   6   7    8   9   :   ;   <   =   >   ?  */
   17, 17, 17, 17, 17, 17, 17, 17,  17, 17, 32, 32, 32, 32, 32, 32,

/*  @   A   B   C   D   E   F   G    H   I   J   K   L   M   N   O  */
   32, 17, 17, 17, 17, 17, 17, 17,  17, 17, 17, 17, 17, 17, 17, 17,

/*  P   Q   R   S   T   U   V   W    X   Y   Z   [   \   ]   ^   _  */
   17, 17, 17, 17, 17, 17, 17, 17,  17, 17, 17, 32, 32, 32, 32, 17,

/*  `   a   b   c   d   e   f   g    h   i   j   k   l   m   n   o  */
   32, 18, 18, 18, 18, 18, 18, 18,  18, 18, 18, 18, 18, 18, 18, 18,

/*  p   q   r   s   t   u   v   w    x   y   z   {   |   }   ~  DEL */
   18, 18, 18, 18, 18, 18, 18, 18,  18, 18, 18, 32, 32, 32, 32, 64,

   64, 64, 64, 64, 64, 64, 64, 64,  64, 64, 64, 64, 64, 64, 64, 64,
   64, 64, 64, 64, 64, 64, 64, 64,  64, 64, 64, 64, 64, 64, 64, 64,
   64, 16, 16, 16, 16, 16, 16, 16,  16, 16, 16, 16, 16, 16, 16, 16,
   16, 16, 16, 16, 16, 16, 16, 16,  16, 16, 16, 16, 16, 16, 16, 16,
   16, 16, 16, 16, 16, 16, 16, 16,  16, 16, 16, 16, 16, 16, 16, 16,
   16, 16, 16, 16, 16, 16, 16, 16,  16, 16, 16, 16, 16, 16, 16, 16,
   16, 16, 16, 16, 16, 16, 16, 16,  16, 16, 16, 16, 16, 16, 16, 16,
   16, 16, 16, 16, 16, 16, 16, 16,  16, 16, 16, 16, 16, 16, 16, 64
};


/* 2004-11-23 SMS.
 *
 *       get_rms_defaults().
 *
 *    Get user-specified values from (DCL) SET RMS_DEFAULT.  FAB/RAB
 *    items of particular interest are:
 *
 *       fab$w_deq         default extension quantity (blocks) (write).
 *       rab$b_mbc         multi-block count.
 *       rab$b_mbf         multi-buffer count (used with rah and wbh).
 */

#define DIAG_FLAG (uO.vflag >= 3)

/* Default RMS parameter values.
 * The default extend quantity (deq) should not matter much here, as the
 * initial allocation should always be set according to the known file
 * size, and no extension should be needed.
 */

#define RMS_DEQ_DEFAULT 16384   /* About 1/4 the max (65535 blocks). */
#define RMS_MBC_DEFAULT 127     /* The max, */
#define RMS_MBF_DEFAULT 2       /* Enough to enable rah and wbh. */

/* GETJPI item descriptor structure. */
typedef struct
{
    short buf_len;
    short itm_cod;
    void *buf;
    int *ret_len;
} jpi_item_t;

/* Durable storage */
static int rms_defaults_known = 0;

/* JPI item buffers. */
static unsigned short rms_ext;
static char rms_mbc;
static unsigned char rms_mbf;

/* Active RMS item values. */
unsigned short rms_ext_active;
char rms_mbc_active;
unsigned char rms_mbf_active;

/* GETJPI item lengths. */
static int rms_ext_len;         /* Should come back 2. */
static int rms_mbc_len;         /* Should come back 1. */
static int rms_mbf_len;         /* Should come back 1. */

/* Desperation attempts to define unknown macros.  Probably doomed.
 * If these get used, expect sys$getjpiw() to return %x00000014 =
 * %SYSTEM-F-BADPARAM, bad parameter value.
 * They keep compilers with old header files quiet, though.
 */
#ifndef JPI$_RMS_EXTEND_SIZE
#  define JPI$_RMS_EXTEND_SIZE 542
#endif /* ndef JPI$_RMS_EXTEND_SIZE */

#ifndef JPI$_RMS_DFMBC
#  define JPI$_RMS_DFMBC 535
#endif /* ndef JPI$_RMS_DFMBC */

#ifndef JPI$_RMS_DFMBFSDK
#  define JPI$_RMS_DFMBFSDK 536
#endif /* ndef JPI$_RMS_DFMBFSDK */

/* GETJPI item descriptor set. */

struct
{
    jpi_item_t rms_ext_itm;
    jpi_item_t rms_mbc_itm;
    jpi_item_t rms_mbf_itm;
    int term;
} jpi_itm_lst =
     { { 2, JPI$_RMS_EXTEND_SIZE, &rms_ext, &rms_ext_len },
       { 1, JPI$_RMS_DFMBC, &rms_mbc, &rms_mbc_len },
       { 1, JPI$_RMS_DFMBFSDK, &rms_mbf, &rms_mbf_len },
       0
     };

static int get_rms_defaults()
{
    int sts;

    /* Get process RMS_DEFAULT values. */

    sts = sys$getjpiw(0, 0, 0, &jpi_itm_lst, 0, 0, 0);
    if ((sts & STS$M_SEVERITY) != STS$K_SUCCESS)
    {
        /* Failed.  Don't try again. */
        rms_defaults_known = -1;
    }
    else
    {
        /* Fine, but don't come back. */
        rms_defaults_known = 1;
    }

    /* Limit the active values according to the RMS_DEFAULT values. */

    if (rms_defaults_known > 0)
    {
        /* Set the default values. */
        rms_ext_active = RMS_DEQ_DEFAULT;
        rms_mbc_active = RMS_MBC_DEFAULT;
        rms_mbf_active = RMS_MBF_DEFAULT;

        /* Default extend quantity.  Use the user value, if set. */
        if (rms_ext > 0)
        {
            rms_ext_active = rms_ext;
        }

        /* Default multi-block count.  Use the user value, if set. */
        if (rms_mbc > 0)
        {
            rms_mbc_active = rms_mbc;
        }

        /* Default multi-buffer count.  Use the user value, if set. */
        if (rms_mbf > 0)
        {
            rms_mbf_active = rms_mbf;
        }
    }

    if (DIAG_FLAG)
    {
        fprintf(stderr, "Get RMS defaults.  getjpi sts = %%x%08x.\n", sts);

        if (rms_defaults_known > 0)
        {
            fprintf(stderr,
              "               Default: deq = %6d, mbc = %3d, mbf = %3d.\n",
              rms_ext, rms_mbc, rms_mbf);
        }
    }
    return sts;
}


int check_format(__G)
    __GDEF
{
    int rtype;
    int sts;
    struct FAB fab;
#ifdef NAML$C_MAXRSS
    struct NAML nam;
#endif

    fab = cc$rms_fab;                   /* Initialize FAB. */

#ifdef NAML$C_MAXRSS

    nam = cc$rms_naml;                  /* Initialize NAML. */
    fab.fab$l_naml = &nam;              /* Point FAB to NAML. */

    fab.fab$l_dna = (char *) -1;    /* Using NAML for default name. */
    fab.fab$l_fna = (char *) -1;    /* Using NAML for file name. */

#endif /* NAML$C_MAXRSS */

    FAB_OR_NAML(fab, nam).FAB_OR_NAML_FNA = G.zipfn;
    FAB_OR_NAML(fab, nam).FAB_OR_NAML_FNS = strlen(G.zipfn);

    if (ERR(sts = sys$open(&fab)))
    {
        Info(slide, 1, ((char *)slide, "\n\
     error:  cannot open zipfile [ %s ].\n",
          FnFilter1(G.zipfn)));
        vms_msg(__G__ "     sys$open() error: ", sts);
        return PK_ERR;
    }
    rtype = fab.fab$b_rfm;
    sys$close(&fab);

    if (rtype == FAB$C_VAR || rtype == FAB$C_VFC)
    {
        Info(slide, 1, ((char *)slide, "\n\
     Error:  zipfile is in variable-length record format.  Please\n\
     run \"bilf l %s\" to convert the zipfile to stream-LF\n\
     record format.  (BILF is available at various VMS archives.)\n\n",
          FnFilter1(G.zipfn)));
        return PK_ERR;
    }

    return PK_COOL;
}



#define PRINTABLE_FORMAT(x)      ( (x) == FAB$C_VAR     \
                                || (x) == FAB$C_STMLF   \
                                || (x) == FAB$C_STMCR   \
                                || (x) == FAB$C_STM     )

/* VMS extra field types */
#define VAT_NONE    0
#define VAT_IZ      1   /* old Info-ZIP format */
#define VAT_PK      2   /* PKWARE format */

/*
 *  open_outfile() assignments:
 *
 *  VMS attributes ?        create_xxx      _flush_xxx
 *  ----------------        ----------      ----------
 *  not found               'default'       text mode ?
 *                                          yes -> 'stream'
 *                                          no  -> 'block'
 *
 *  yes, in IZ format       'rms'           uO.cflag ?
 *                                          yes -> switch (fab.rfm)
 *                                              VAR  -> 'varlen'
 *                                              STM* -> 'stream'
 *                                              default -> 'block'
 *                                          no -> 'block'
 *
 *  yes, in PK format       'qio'           uO.cflag ?
 *                                          yes -> switch (pka_rattr)
 *                                              VAR  -> 'varlen'
 *                                              STM* -> 'stream'
 *                                              default -> 'block'
 *                                          no -> 'qio'
 *
 *  "text mode" == G.pInfo->textmode || (uO.cflag && !uO.bflag)
 *  (simplified, for complete expression see create_default_output() code)
 */

/* The VMS version of open_outfile() supports special return codes:
 *      OPENOUT_OK            a file has been opened normally
 *      OPENOUT_FAILED        the file open process failed
 *      OPENOUT_SKIPOK        file open skipped at user request, err level OK
 *      OPENOUT_SKIPWARN      file open skipped at user request, err level WARN
 */
int open_outfile(__G)
    __GDEF
{
    /* Get process RMS_DEFAULT values, if not already done. */
    if (rms_defaults_known == 0)
    {
        get_rms_defaults();
    }

    switch (find_vms_attrs(__G__ (uO.D_flag <= 1)))
    {
        case VAT_NONE:
        default:
            return  create_default_output(__G);
        case VAT_IZ:
            return  create_rms_output(__G);
        case VAT_PK:
            return  create_qio_output(__G);
    }
}

static void init_buf_ring()
{
    locptr = &locbuf[0];
    loccnt = 0;

    b1.buf = &locbuf[0];
    b1.bufcnt = 0;
    b1.next = &b2;
    b2.buf = &locbuf[BUFS512];
    b2.bufcnt = 0;
    b2.next = &b1;
    curbuf = &b1;
}


/* Static data storage for time conversion: */

/*   string constants for month names */
static ZCONST char *month[] =
            {"JAN", "FEB", "MAR", "APR", "MAY", "JUN",
             "JUL", "AUG", "SEP", "OCT", "NOV", "DEC"};

/*   buffer for time string */
static char timbuf[24];         /* length = first entry in "date_str" + 1 */

/*   fixed-length string descriptor for timbuf: */
static ZCONST struct dsc$descriptor date_str =
            {sizeof(timbuf)-1, DSC$K_DTYPE_T, DSC$K_CLASS_S, timbuf};


static void set_default_datetime_XABs(__GPRO)
{
    unsigned yr, mo, dy, hh, mm, ss;
#ifdef USE_EF_UT_TIME
    iztimes z_utime;
    struct tm *t;

    if (G.extra_field &&
#ifdef IZ_CHECK_TZ
        G.tz_is_valid &&
#endif
        (ef_scan_for_izux(G.extra_field, G.lrec.extra_field_length, 0,
                          G.lrec.last_mod_dos_datetime, &z_utime, NULL)
         & EB_UT_FL_MTIME))
        t = localtime(&(z_utime.mtime));
    else
        t = (struct tm *)NULL;
    if (t != (struct tm *)NULL)
    {
        yr = t->tm_year + 1900;
        mo = t->tm_mon;
        dy = t->tm_mday;
        hh = t->tm_hour;
        mm = t->tm_min;
        ss = t->tm_sec;
    }
    else
    {
        yr = ((G.lrec.last_mod_dos_datetime >> 25) & 0x7f) + 1980;
        mo = ((G.lrec.last_mod_dos_datetime >> 21) & 0x0f) - 1;
        dy = (G.lrec.last_mod_dos_datetime >> 16) & 0x1f;
        hh = (G.lrec.last_mod_dos_datetime >> 11) & 0x1f;
        mm = (G.lrec.last_mod_dos_datetime >> 5) & 0x3f;
        ss = (G.lrec.last_mod_dos_datetime << 1) & 0x3e;
    }
#else /* !USE_EF_UT_TIME */

    yr = ((G.lrec.last_mod_dos_datetime >> 25) & 0x7f) + 1980;
    mo = ((G.lrec.last_mod_dos_datetime >> 21) & 0x0f) - 1;
    dy = (G.lrec.last_mod_dos_datetime >> 16) & 0x1f;
    hh = (G.lrec.last_mod_dos_datetime >> 11) & 0x1f;
    mm = (G.lrec.last_mod_dos_datetime >> 5) & 0x3f;
    ss = (G.lrec.last_mod_dos_datetime << 1) & 0x1f;
#endif /* ?USE_EF_UT_TIME */

    dattim = cc$rms_xabdat;     /* fill XABs with default values */
    rdt = cc$rms_xabrdt;
    sprintf(timbuf, "%02u-%3s-%04u %02u:%02u:%02u.00",
            dy, month[mo], yr, hh, mm, ss);
    sys$bintim(&date_str, &dattim.xab$q_cdt);
    memcpy(&rdt.xab$q_rdt, &dattim.xab$q_cdt, sizeof(rdt.xab$q_rdt));
}


/* The following return codes are supported:
 *      OPENOUT_OK            a file has been opened normally
 *      OPENOUT_FAILED        the file open process failed
 *      OPENOUT_SKIPOK        file open skipped at user request, err level OK
 *      OPENOUT_SKIPWARN      file open skipped at user request, err level WARN
 */
static int create_default_output(__GPRO)
{
    int ierr;
    int text_output, bin_fixed;

    /* Extract the file in text format (Variable_length by default,
     * Stream_LF with "-S" (/TEXT = STMLF), when
     *  a) explicitly requested by the user (through the -a option),
     *     and it is not a symbolic link,
     * or
     *  b) piping to SYS$OUTPUT, unless "binary" piping was requested
     *     by the user (through the -b option).
     */
    text_output = (G.pInfo->textmode
#ifdef SYMLINKS
                   && !G.symlnk
#endif
                  ) ||
                  (uO.cflag &&
                   (!uO.bflag || (!(uO.bflag - 1) && G.pInfo->textfile)));
    /* Use fixed length 512 byte record format for disk file when
     *  a) explicitly requested by the user (-b option),
     * and
     *  b) it is not a symbolic link,
     * and
     *  c) it is not extracted in text mode.
     */
    bin_fixed = !text_output &&
#ifdef SYMLINKS
                !G.symlnk &&
#endif
                (uO.bflag != 0) && ((uO.bflag != 1) || !G.pInfo->textfile);

    rfm = FAB$C_STMLF;  /* Default, stream-LF format from VMS or UNIX */

    if (!uO.cflag)              /* Redirect output */
    {
        rab = cc$rms_rab;               /* Initialize RAB. */
        fileblk = cc$rms_fab;           /* Initialize FAB. */

        fileblk.fab$l_xab = NULL;       /* No XABs. */
        rab.rab$l_fab = &fileblk;       /* Point RAB to FAB. */

        outfab = &fileblk;              /* Set pointers used elsewhere. */
        outrab = &rab;

        if (text_output && (!uO.S_flag))
        {   /* Default format for output `real' text file */
            fileblk.fab$b_rfm = FAB$C_VAR;      /* variable length records */
            fileblk.fab$b_rat = FAB$M_CR;       /* implied (CR) carriage ctrl */
        }
        else if (bin_fixed)
        {   /* Default format for output `real' binary file */
            fileblk.fab$b_rfm = FAB$C_FIX;      /* fixed length records */
            fileblk.fab$w_mrs = 512;            /* record size 512 bytes */
            fileblk.fab$b_rat = 0;              /* no carriage ctrl */
        }
        else
        {   /* Default format for output misc (bin or text) file */
            fileblk.fab$b_rfm = FAB$C_STMLF;    /* stream-LF record format */
            fileblk.fab$b_rat = FAB$M_CR;       /* implied (CR) carriage ctrl */
        }

#ifdef NAML$C_MAXRSS

        nam = CC_RMS_NAM;               /* Initialize NAML. */
        fileblk.FAB_NAM = &nam;         /* Point FAB to NAML. */

        fileblk.fab$l_dna = (char *) -1;    /* Using NAML for default name. */
        fileblk.fab$l_fna = (char *) -1;    /* Using NAML for file name. */

#endif /* NAML$C_MAXRSS */

        FAB_OR_NAML(fileblk, nam).FAB_OR_NAML_FNA = G.filename;
        FAB_OR_NAML(fileblk, nam).FAB_OR_NAML_FNS = strlen(G.filename);

        /* Prepare date-time XABs, unless user requests not to. */
        if (uO.D_flag <= 1) {
            set_default_datetime_XABs(__G);
            dattim.xab$l_nxt = fileblk.fab$l_xab;
            fileblk.fab$l_xab = (void *) &dattim;
        }

/* 2005-02-14 SMS.  What does this mean?  ----vvvvvvvvvvvvvvvvvvvvvvvvvvv */
        fileblk.fab$w_ifi = 0;  /* Clear IFI. It may be nonzero after ZIP */
        fileblk.fab$b_fac = FAB$M_BRO | FAB$M_PUT;  /* {block|record} output */
#ifdef SYMLINKS
        if (G.symlnk)
            /* Symlink file is read back to retrieve the link text. */
            fileblk.fab$b_fac |= FAB$M_GET;
#endif

        /* 2004-11-23 SMS.
         * If RMS_DEFAULT values have been determined, and have not been
         * set by the user, then set some FAB/RAB parameters for faster
         * output.  User-specified RMS_DEFAULT values override the
         * built-in default values, so if the RMS_DEFAULT values could
         * not be determined, then these (possibly unwise) values could
         * not be overridden, and hence will not be set.  Honestly,
         * this seems to be excessively cautious, but only old VMS
         * versions will be affected.
         */

        /* If RMS_DEFAULT (and adjusted active) values are available,
         * then set the FAB/RAB parameters.  If RMS_DEFAULT values are
         * not available, then suffer with the default behavior.
         */
        if (rms_defaults_known > 0)
        {
            /* Set the FAB/RAB parameters accordingly. */
            fileblk.fab$w_deq = rms_ext_active;
            rab.rab$b_mbc = rms_mbc_active;
            rab.rab$b_mbf = rms_mbf_active;

#ifdef OLD_FABDEF

            /* Truncate at EOF on close, as we may over-extend. */
            fileblk.fab$l_fop |= FAB$M_TEF ;

            /* If using multiple buffers, enable write-behind. */
            if (rms_mbf_active > 1)
            {
                rab.rab$l_rop |= RAB$M_WBH;
            }
        }

        /* Set the initial file allocation according to the file
         * size.  Also set the "sequential access only" flag, as
         * otherwise, on a file system with highwater marking
         * enabled, allocating space for a large file may lock the
         * disk for a long time (minutes).
         */
        fileblk.fab$l_alq = (unsigned) (G.lrec.ucsize+ 511)/ 512;
        fileblk.fab$l_fop |= FAB$M_SQO;

#else /* !OLD_FABDEF */

            /* Truncate at EOF on close, as we may over-extend. */
            fileblk.fab$v_tef = 1;

            /* If using multiple buffers, enable write-behind. */
            if (rms_mbf_active > 1)
            {
                rab.rab$v_wbh = 1;
            }
        }

        /* Set the initial file allocation according to the file
         * size.  Also set the "sequential access only" flag, as
         * otherwise, on a file system with highwater marking
         * enabled, allocating space for a large file may lock the
         * disk for a long time (minutes).
         */
        fileblk.fab$l_alq = (unsigned) (G.lrec.ucsize+ 511)/ 512;
        fileblk.fab$v_sqo = 1;

#endif /* ?OLD_FABDEF */

        ierr = sys$create(outfab);
        if (ierr == RMS$_FEX)
        {
            /* File exists.
             * Consider command-line options, or ask the user what to do.
             */
            ierr = replace(__G);
            switch (ierr & REPL_TASKMASK)
            {
                case REPL_NO_EXTRACT:   /* No extract. */
                    free_up();
                    return ((ierr & REPL_ERRLV_WARN)
                            ? OPENOUT_SKIPWARN : OPENOUT_SKIPOK);
                case REPL_NEW_VERSION:  /* Create a new version. */
                    ierr = replace_rms_newversion(__G);
                    break;
                case REPL_OVERWRITE:    /* Overwrite the existing file. */
                    ierr = replace_rms_overwrite(__G);
                    break;
            }
        }

        if (ERR(ierr))
        {
            char buf[NAM_MAXRSS + 128]; /* Name length + message length. */

            sprintf(buf, "[ Cannot create ($create) output file %s ]\n",
              G.filename);
            vms_msg(__G__ buf, ierr);
            if (fileblk.fab$l_stv != 0)
            {
                vms_msg(__G__ "", fileblk.fab$l_stv);
            }
            free_up();
            return OPENOUT_FAILED;
        }

        if (!text_output)
        {
            rab.rab$l_rop |= (RAB$M_BIO | RAB$M_ASY);
        }
        rab.rab$b_rac = RAB$C_SEQ;

        if ((ierr = sys$connect(&rab)) != RMS$_NORMAL)
        {
#ifdef DEBUG
            vms_msg(__G__ "create_default_output: sys$connect failed.\n", ierr);
            if (fileblk.fab$l_stv != 0)
            {
                vms_msg(__G__ "", fileblk.fab$l_stv);
            }
#endif
            Info(slide, 1, ((char *)slide,
                 "Cannot create ($connect) output file:  %s\n",
                 FnFilter1(G.filename)));
            free_up();
            return OPENOUT_FAILED;
        }
    }                   /* end if (!uO.cflag) */

    init_buf_ring();

    _flush_routine = text_output ? got_eol=0,_flush_stream : _flush_blocks;
    _close_routine = _close_rms;
    return OPENOUT_OK;
}



/* The following return codes are supported:
 *      OPENOUT_OK            a file has been opened normally
 *      OPENOUT_FAILED        the file open process failed
 *      OPENOUT_SKIPOK        file open skipped at user request, err level OK
 *      OPENOUT_SKIPWARN      file open skipped at user request, err level WARN
 */
static int create_rms_output(__GPRO)
{
    int ierr;
    int text_output;

    /* extract the file in text (variable-length) format, when
     * piping to SYS$OUTPUT, unless "binary" piping was requested
     * by the user (through the -b option); the "-a" option is
     * ignored when extracting zip entries with VMS attributes saved
     */
    text_output = uO.cflag &&
                  (!uO.bflag || (!(uO.bflag - 1) && G.pInfo->textfile));

    rfm = outfab->fab$b_rfm;    /* Use record format from VMS extra field */

    if (uO.cflag)               /* SYS$OUTPUT */
    {
        if (text_output && !PRINTABLE_FORMAT(rfm))
        {
            Info(slide, 1, ((char *)slide,
               "[ File %s has illegal record format to put to screen ]\n",
               FnFilter1(G.filename)));
            free_up();
            return OPENOUT_FAILED;
        }
    }
    else                        /* File output */
    {
        rab = cc$rms_rab;               /* Initialize RAB. */

        /* The output FAB has already been initialized with the values
         * found in the Zip file's "VMS attributes" extra field.
         */

#ifdef NAML$C_MAXRSS

        nam = CC_RMS_NAM;               /* Initialize NAML. */
        outfab->FAB_NAM = &nam;         /* Point FAB to NAML. */

        outfab->fab$l_dna = (char *) -1;    /* Using NAML for default name. */
        outfab->fab$l_fna = (char *) -1;    /* Using NAML for file name. */

#endif /* NAML$C_MAXRSS */

        FAB_OR_NAML(*outfab, nam).FAB_OR_NAML_FNA = G.filename;
        FAB_OR_NAML(*outfab, nam).FAB_OR_NAML_FNS = strlen(G.filename);

        /* Prepare date-time XABs, unless user requests not to. */
        if (uO.D_flag <= 1) {
            /* If no XAB date/time, use attributes from non-VMS fields. */
            if (!(xabdat && xabrdt))
            {
                set_default_datetime_XABs(__G);

                if (xabdat == NULL)
                {
                    dattim.xab$l_nxt = outfab->fab$l_xab;
                    outfab->fab$l_xab = (void *) &dattim;
                }
            }
        }
/* 2005-02-14 SMS.  What does this mean?  ----vvvvvvvvvvvvvvvvvvvvvvvvvvv */
        outfab->fab$w_ifi = 0;  /* Clear IFI. It may be nonzero after ZIP */
        outfab->fab$b_fac = FAB$M_BIO | FAB$M_PUT;      /* block-mode output */
#ifdef SYMLINKS
        /* 2007-02-28 SMS.
         * VMS/RMS symlink properties will be restored naturally when
         * the link file is recreated this way, so there's no need to do
         * the deferred symlink post-processing step for this file.
         * Therefore, clear the pInfo->symlink flag here, and the symlink
         * "close file" processor will only display the link text.
         */
        if (G.symlnk) {
            G.pInfo->symlink = 0;
            if (QCOND2) {
                /* Symlink file is read back to display the link text. */
                outfab->fab$b_fac |= FAB$M_GET;
            }
        }
#endif /* SYMLINKS */

        /* 2004-11-23 SMS.
         * Set the "sequential access only" flag, as otherwise, on a
         * file system with highwater marking enabled, allocating space
         * for a large file may lock the disk for a long time (minutes).
         */
#ifdef OLD_FABDEF
        outfab-> fab$l_fop |= FAB$M_SQO;
#else /* !OLD_FABDEF */
        outfab-> fab$v_sqo = 1;
#endif /* ?OLD_FABDEF */

        ierr = sys$create(outfab);
        if (ierr == RMS$_FEX)
        {
            /* File exists.
             * Consider command-line options, or ask the user what to do.
             */
            ierr = replace(__G);
            switch (ierr & REPL_TASKMASK)
            {
                case REPL_NO_EXTRACT:   /* No extract. */
                    free_up();
                    return ((ierr & REPL_ERRLV_WARN)
                            ? OPENOUT_SKIPWARN : OPENOUT_SKIPOK);
                case REPL_NEW_VERSION:  /* Create a new version. */
                    ierr = replace_rms_newversion(__G);
                    break;
                case REPL_OVERWRITE:    /* Overwrite the existing file. */
                    ierr = replace_rms_overwrite(__G);
                    break;
            }
        }

        if (ERR(ierr))
        {
            char buf[NAM_MAXRSS + 128]; /* Name length + message length. */

            sprintf(buf, "[ Cannot create ($create) output file %s ]\n",
              G.filename);
            vms_msg(__G__ buf, ierr);
            if (outfab->fab$l_stv != 0)
            {
                vms_msg(__G__ "", outfab->fab$l_stv);
            }
            free_up();
            return OPENOUT_FAILED;
        }

        if (outfab->fab$b_org & (FAB$C_REL | FAB$C_IDX)) {
            /* relative and indexed files require explicit allocation */
            ierr = sys$extend(outfab);
            if (ERR(ierr))
            {
                char buf[NAM_MAXRSS + 128];    /* Name length + msg length. */

                sprintf(buf, "[ Cannot allocate space for %s ]\n", G.filename);
                vms_msg(__G__ buf, ierr);
                if (outfab->fab$l_stv != 0)
                {
                    vms_msg(__G__ "", outfab->fab$l_stv);
                }
                free_up();
                return OPENOUT_FAILED;
            }
        }

        outrab = &rab;
        rab.rab$l_fab = outfab;
        {
            rab.rab$l_rop |= (RAB$M_BIO | RAB$M_ASY);
        }
        rab.rab$b_rac = RAB$C_SEQ;

        if ((ierr = sys$connect(outrab)) != RMS$_NORMAL)
        {
#ifdef DEBUG
            vms_msg(__G__ "create_rms_output: sys$connect failed.\n", ierr);
            if (outfab->fab$l_stv != 0)
            {
                vms_msg(__G__ "", outfab->fab$l_stv);
            }
#endif
            Info(slide, 1, ((char *)slide,
                 "Cannot create ($connect) output file:  %s\n",
                 FnFilter1(G.filename)));
            free_up();
            return OPENOUT_FAILED;
        }
    }                   /* end if (!uO.cflag) */

    init_buf_ring();

    if ( text_output )
        switch (rfm)
        {
            case FAB$C_VAR:
                _flush_routine = _flush_varlen;
                break;
            case FAB$C_STM:
            case FAB$C_STMCR:
            case FAB$C_STMLF:
                _flush_routine = _flush_stream;
                got_eol = 0;
                break;
            default:
                _flush_routine = _flush_blocks;
                break;
        }
    else
        _flush_routine = _flush_blocks;
    _close_routine = _close_rms;
    return OPENOUT_OK;
}



static  int pka_devchn;
static  int pka_io_pending;
static  unsigned pka_vbn;

/* IOSB for QIO[W] read and write operations. */
#if defined(__DECC) || defined(__DECCXX)
#pragma __member_alignment __save
#pragma __nomember_alignment
#endif /* __DECC || __DECCXX */
static struct
{
    unsigned short  status;
    unsigned int    count;      /* Unaligned ! */
    unsigned short  dummy;
} pka_io_iosb;
#if defined(__DECC) || defined(__DECCXX)
#pragma __member_alignment __restore
#endif /* __DECC || __DECCXX */

/* IOSB for QIO[W] miscellaneous ACP operations. */
static struct
{
    unsigned short  status;
    unsigned short  dummy;
    unsigned int    count;
} pka_acp_iosb;

static struct fibdef    pka_fib;
static struct atrdef    pka_atr[VMS_MAX_ATRCNT];
static int              pka_idx;
static ulg              pka_uchar;
static struct fatdef    pka_rattr;

/* Directory attribute storage, descriptor (list). */
static struct atrdef pka_recattr[2] =
 { { sizeof(pka_rattr), ATR$C_RECATTR, GVTC &pka_rattr},        /* RECATTR. */
   { 0, 0, 0 }                                          /* List terminator. */
 };

static struct dsc$descriptor    pka_fibdsc =
{   sizeof(pka_fib), DSC$K_DTYPE_Z, DSC$K_CLASS_S, (void *) &pka_fib  };

static struct dsc$descriptor_s  pka_devdsc =
{   0, DSC$K_DTYPE_T, DSC$K_CLASS_S, &nam.NAM_DVI[1]  };

static struct dsc$descriptor_s  pka_fnam =
{   0, DSC$K_DTYPE_T, DSC$K_CLASS_S, NULL  };

/* Expanded and resultant name storage. */
static char exp_nam[NAM_MAXRSS];
static char res_nam[NAM_MAXRSS];

/* Special ODS5-QIO-compatible name storage. */
#ifdef NAML$C_MAXRSS
static char sys_nam[NAML$C_MAXRSS];     /* Probably need less here. */
#endif /* NAML$C_MAXRSS */

#define PK_PRINTABLE_RECTYP(x)   ( (x) == FAT$C_VARIABLE \
                                || (x) == FAT$C_STREAMLF \
                                || (x) == FAT$C_STREAMCR \
                                || (x) == FAT$C_STREAM   )


/* The following return codes are supported:
 *      OPENOUT_OK            a file has been opened normally
 *      OPENOUT_FAILED        the file open process failed
 *      OPENOUT_SKIPOK        file open skipped at user request, err level OK
 *      OPENOUT_SKIPWARN      file open skipped at user request, err level WARN
 */
static int create_qio_output(__GPRO)
{
    int status;
    int i;
    int text_output;

    /* extract the file in text (variable-length) format, when
     * piping to SYS$OUTPUT, unless "binary" piping was requested
     * by the user (through the -b option); the "-a" option is
     * ignored when extracting zip entries with VMS attributes saved
     */
    text_output = uO.cflag &&
                  (!uO.bflag || (!(uO.bflag - 1) && G.pInfo->textfile));

    if ( uO.cflag )
    {
        int rtype;

        if (text_output)
        {
            rtype = pka_rattr.fat$v_rtype;
            if (!PK_PRINTABLE_RECTYP(rtype))
            {
                Info(slide, 1, ((char *)slide,
                   "[ File %s has illegal record format to put to screen ]\n",
                   FnFilter1(G.filename)));
                return OPENOUT_FAILED;
            }
        }
        else
            /* force "block I/O" for binary piping mode */
            rtype = FAT$C_UNDEFINED;

        init_buf_ring();

        switch (rtype)
        {
            case FAT$C_VARIABLE:
                _flush_routine = _flush_varlen;
                break;
            case FAT$C_STREAM:
            case FAT$C_STREAMCR:
            case FAT$C_STREAMLF:
                _flush_routine = _flush_stream;
                got_eol = 0;
                break;
            default:
                _flush_routine = _flush_blocks;
                break;
        }
        _close_routine = _close_rms;
    }
    else                        /* !(uO.cflag) : redirect output */
    {
        fileblk = cc$rms_fab;           /* Initialize FAB. */
        nam = CC_RMS_NAM;               /* Initialize NAM[L]. */
        fileblk.FAB_NAM = &nam;         /* Point FAB to NAM[L]. */

#ifdef NAML$C_MAXRSS

        fileblk.fab$l_dna = (char *) -1;    /* Using NAML for default name. */
        fileblk.fab$l_fna = (char *) -1;    /* Using NAML for file name. */

        /* Special ODS5-QIO-compatible name storage. */
        nam.naml$l_filesys_name = sys_nam;
        nam.naml$l_filesys_name_alloc = sizeof(sys_nam);

#endif /* NAML$C_MAXRSS */

        /* VMS-format file name, derived from archive. */
        FAB_OR_NAML(fileblk, nam).FAB_OR_NAML_FNA = G.filename;
        FAB_OR_NAML(fileblk, nam).FAB_OR_NAML_FNS = strlen(G.filename);

        /* Expanded and resultant name storage. */
        nam.NAM_ESA = exp_nam;
        nam.NAM_ESS = sizeof(exp_nam);
        nam.NAM_RSA = res_nam;
        nam.NAM_RSS = sizeof(res_nam);

        if ( ERR(status = sys$parse(&fileblk)) )
        {
            vms_msg(__G__ "create_qio_output: sys$parse failed.\n", status);
            return OPENOUT_FAILED;
        }

        pka_devdsc.dsc$w_length = (unsigned short)nam.NAM_DVI[0];

        if ( ERR(status = sys$assign(&pka_devdsc, &pka_devchn, 0, 0)) )
        {
            vms_msg(__G__ "create_qio_output: sys$assign failed.\n", status);
            return OPENOUT_FAILED;
        }

#ifdef NAML$C_MAXRSS

        /* Enable fancy name characters.  Note that "fancy" here does
           not include Unicode, for which there's no support elsewhere.
        */
        pka_fib.fib$v_names_8bit = 1;
        pka_fib.fib$b_name_format_in = FIB$C_ISL1;

        /* ODS5 Extended names used as input to QIO have peculiar
           encoding (perhaps to minimize storage?), so the special
           filesys_name result (typically containing fewer carets) must
           be used here.
        */
        pka_fnam.dsc$a_pointer = nam.naml$l_filesys_name;
        pka_fnam.dsc$w_length = nam.naml$l_filesys_name_size;

#else /* !NAML$C_MAXRSS */

        /* Extract only the name.type;version.
           2005-02-14 SMS.
           Note: In old code, the version in the name here was retained
           only if -V (uO.V_flag, so that there might be an explicit
           version number in the archive (or perhaps not)), but the
           version should already have been stripped before this in
           adj_file_name_odsX(), and sys$parse() here should always
           return a good version number which may be used as-is.  If
           not, here's where to fix the (new) problem.  Note that the
           ODS5-compatible code uses the whole thing in filesys_name,
           too, and that's critical for proper interpretation of funny
           names.  (Omitting the ";" can cause trouble, so it should
           certainly be kept, even if the version digits are removed
           here.)
        */

        pka_fnam.dsc$a_pointer = nam.NAM_L_NAME;
        pka_fnam.dsc$w_length =
          nam.NAM_B_NAME + nam.NAM_B_TYPE + nam.NAM_B_VER;

#if 0
        pka_fnam.dsc$w_length = nam.NAM_B_NAME + nam.NAM_B_TYPE;
        if ( uO.V_flag /* keep versions */ )
            pka_fnam.dsc$w_length += nam.NAM_B_VER;
#endif /* 0 */

#endif /* ?NAML$C_MAXRSS */

        /* Move the directory ID from the NAM[L] to the FIB.
           Clear the FID in the FIB, as we're using the name.
        */
        for (i = 0; i < 3; i++)
        {
            pka_fib.FIB$W_DID[i] = nam.NAM_DID[i];
            pka_fib.FIB$W_FID[i] = 0;
        }

        /* 2004-11-23 SMS.
         * Set the "sequential access only" flag, as otherwise, on a
         * file system with highwater marking enabled, allocating space
         * for a large file may lock the disk for a long time (minutes).
         * (The "no other readers" flag is also required, if you want
         * the "sequential access only" flag to have any effect.)
         */
        pka_fib.FIB$L_ACCTL = FIB$M_WRITE | FIB$M_SEQONLY | FIB$M_NOREAD;

        /* Allocate space for the file */
        pka_fib.FIB$W_EXCTL = FIB$M_EXTEND;
        if ( pka_uchar & FCH$M_CONTIG )
            pka_fib.FIB$W_EXCTL |= FIB$M_ALCON | FIB$M_FILCON;
        if ( pka_uchar & FCH$M_CONTIGB )
            pka_fib.FIB$W_EXCTL |= FIB$M_ALCONB;

#define SWAPW(x)        ( (((x)>>16)&0xFFFF) + ((x)<<16) )

        pka_fib.fib$l_exsz = SWAPW(pka_rattr.fat$l_hiblk);

        status = sys$qiow(0,                /* event flag */
                          pka_devchn,       /* channel */
                          IO$_CREATE|IO$M_CREATE|IO$M_ACCESS, /* funct */
                          &pka_acp_iosb,    /* IOSB */
                          0,                /* AST address */
                          0,                /* AST parameter */
                          &pka_fibdsc,      /* P1 = File Info Block */
                          &pka_fnam,        /* P2 = File name (descr) */
                          0,                /* P3 (= Resulting name len) */
                          0,                /* P4 (= Resulting name descr) */
                          pka_atr,          /* P5 = Attribute descr */
                          0);               /* P6 (not used) */

        if ( !ERR(status) )
            status = pka_acp_iosb.status;

        if ( status == SS$_DUPFILENAME )
        {
            /* File exists.  Prepare to ask user what to do. */

            /* Arrange to store the resultant file spec (with new
             * version?) where the message code will find it.
             */
            short res_nam_len;
            struct dsc$descriptor_s  res_nam_dscr =
              { 0, DSC$K_DTYPE_T, DSC$K_CLASS_S, NULL };

            res_nam_dscr.dsc$a_pointer = G.filename;
            res_nam_dscr.dsc$w_length = sizeof(G.filename);

            /* File exists.
             * Consider command-line options, or ask the user what to do.
             */
            status = replace(__G);
            switch (status & REPL_TASKMASK)
            {
                case REPL_NO_EXTRACT:   /* No extract. */
                    free_up();
                    return ((status & REPL_ERRLV_WARN)
                            ? OPENOUT_SKIPWARN : OPENOUT_SKIPOK);
                case REPL_NEW_VERSION:  /* Create a new version. */
                    pka_fib.FIB$W_NMCTL |= FIB$M_NEWVER;
                    break;
                case REPL_OVERWRITE:    /* Overwrite the existing file. */
                    pka_fib.FIB$W_NMCTL |= FIB$M_SUPERSEDE;
                    break;
            }

            /* Retry file creation with new (user-specified) policy. */
            status = sys$qiow(0,                /* event flag */
                              pka_devchn,       /* channel */
                              IO$_CREATE|IO$M_CREATE|IO$M_ACCESS, /* funct */
                              &pka_acp_iosb,    /* IOSB */
                              0,                /* AST address */
                              0,                /* AST parameter */
                              &pka_fibdsc,      /* P1 = File Info Block */
                              &pka_fnam,        /* P2 = File name (descr) */
                              &res_nam_len,     /* P3 = Resulting name len */
                              &res_nam_dscr,    /* P4 = Resulting name descr */
                              pka_atr,          /* P5 = Attribute descr */
                              0);               /* P6 (not used) */

            if ( !ERR(status) )
                status = pka_acp_iosb.status;

            if (res_nam_len > 0)
            {
                /* NUL-terminate the resulting file spec. */
                G.filename[res_nam_len] = '\0';
            }

            /* Clear any user-specified version policy flags
             * (for the next file to be processed).
             */
            pka_fib.FIB$W_NMCTL &= (~(FIB$M_NEWVER| FIB$M_SUPERSEDE));
        }

        if ( ERR(status) )
        {
            char buf[NAM_MAXRSS + 128]; /* Name length + message length. */

            sprintf(buf, "[ Cannot create (QIO) output file %s ]\n",
              G.filename);
            vms_msg(__G__ buf, status);
            sys$dassgn(pka_devchn);
            return OPENOUT_FAILED;
        }

#ifdef ASYNCH_QIO
        init_buf_ring();
        pka_io_pending = FALSE;
#else
        locptr = locbuf;
        loccnt = 0;
#endif
        pka_vbn = 1;
        _flush_routine = _flush_qio;
        _close_routine = _close_qio;
    }                   /* end if (!uO.cflag) */
    return OPENOUT_OK;
}


/* 2008-07-23 SMS.
 * Segregated user query function from file re-open functions/code.
 *
 * There was no code in create_qio_output() to deal with an
 * SS$_DUPFILENAME condition, leading to ugly run-time failures, and its
 * requirements differ from those of the RMS (non-QIO) functions,
 * create_default_output() and create_rms_output().
 *
 * Whether it makes sense to have a second layer of VMS-specific
 * querying after the generic UnZip query in extract.c:
 * extract_or_test_entrylist() is another question, but changing that
 * looks more scary than just getting the VMS-specific stuff to work
 * right (better?).
 */

/* "File exists" handler(s).  Ask user about further action. */

/* RMS create new version. */
static int replace_rms_newversion(__GPRO)
{
    int ierr;
    struct NAM_STRUCT nam;

    nam = CC_RMS_NAM;           /* Initialize local NAM[L] block. */
    outfab->FAB_NAM = &nam;     /* Point FAB to local NAM[L]. */

    /* Arrange to store the resultant file spec (with new version), so
     * that we can extract the actual file version from it, for later
     * use in the "extracting:/inflating:/..."  message (G.filename).
     */
    nam.NAM_RSA = res_nam;
    nam.NAM_RSS = sizeof(res_nam);

#ifdef NAML$C_MAXRSS

    outfab->fab$l_dna = (char *) -1;    /* Using NAML for default name. */
    outfab->fab$l_fna = (char *) -1;    /* Using NAML for file name. */

#endif /* NAML$C_MAXRSS */

    FAB_OR_NAML(*outfab, nam).FAB_OR_NAML_FNA = G.filename;
    FAB_OR_NAML(*outfab, nam).FAB_OR_NAML_FNS = strlen(G.filename);

    /* Maximize version number. */
    outfab->fab$l_fop |= FAB$M_MXV;

    /* Create the new-version file. */
    ierr = sys$create(outfab);

    if (nam.NAM_RSL > 0)
    {
        /* File spec version pointers.
         * Versions must exist, so a simple right-to-left search for ";"
         * should work, even on ODS5 extended file specs.
         */
        char *semi_col_orig;
        char *semi_col_res;

        /* NUL-terminate the (complete) resultant file spec. */
        res_nam[nam.NAM_RSL] = '\0';

        /* Find the versions (";") in the original and resultant file specs. */
        semi_col_orig = strrchr(G.filename, ';');
        semi_col_res = strrchr(res_nam, ';');

        if ((semi_col_orig != NULL) && (semi_col_res != NULL))
        {
            /* Transfer the resultant version to the original file spec. */
            strcpy((semi_col_orig + 1), (semi_col_res + 1));
        }
    }
    return ierr;
}


/* RMS overwrite original version. */
static int replace_rms_overwrite(__GPRO)
{
    /* Supersede existing file. */
    outfab->fab$l_fop |= FAB$M_SUP;
    /* Create (overwrite) the original-version file. */
    return sys$create(outfab);
}


/* Main query function to ask user how to handle an existing file
 * (unless command-line options already specify what to do).
 */
static int replace(__GPRO)
{
    char answ[10];
    int replace_code;

    if (replace_code_all >= 0)
    {
        /* Use the previous all-file response. */
        replace_code = replace_code_all;
    }
    else if (uO.overwrite_none)
    {
        /* "-n".  Do not extract this (or any) file. */
        replace_code = replace_code_all = REPL_NO_EXTRACT;
    }
    else if (uO.overwrite_all == 1)
    {
        /* "-o".  Create a new version of this (or any) file. */
        replace_code = replace_code_all = REPL_NEW_VERSION;
    }
    else if (uO.overwrite_all > 1)
    {
        /* "-oo".  Overwrite (supersede) this (or any) existing file. */
        replace_code = replace_code_all = REPL_OVERWRITE;
    }
    else
    {
        replace_code = -1;
        do
        {
            /* Request, accept, and decode a response. */
            Info(slide, 0x81, ((char *)slide,
              "%s exists:  new [v]ersion, [o]verwrite, or [n]o extract?\n\
  (Uppercase response [V,O,N] => Do same for all files): ",
              FnFilter1(G.filename)));
            fflush(stderr);

            if (fgets(answ, sizeof(answ), stdin) == (char *)NULL)
            {
                Info(slide, 1, ((char *)slide, AssumeNo));
                /* Handle the NULL answer as "N",
                 * do not extract any existing files.  */
                replace_code_all = REPL_NO_EXTRACT;
                /* Set a warning indicator. */
                replace_code = REPL_NO_EXTRACT | REPL_ERRLV_WARN;
                /* We are finished, break out of the query loop. */
                break;
            }

            /* Strip off a trailing newline, to avoid corrupt
             * complaints when displaying the answer.
             */
            if (answ[strlen(answ) - 1] == '\n')
                answ[strlen(answ) - 1] = '\0';

            /* Extra newline to avoid having the extracting:/inflating:/...:
             * message overwritten by the next query.
             */
            Info(slide, 1, ((char *)slide, "\n"));

            /* Interpret response.  Store upper-case answer for future use. */
            switch (answ[0])
            {
                case 'N':
                    replace_code_all = REPL_NO_EXTRACT;
                case 'n':
                    /* Do not extract this file. */
                    replace_code = REPL_NO_EXTRACT;
                    break;
                case 'O':
                    replace_code_all = REPL_OVERWRITE;
                case 'o':
                    /* Overwrite (supersede) this existing file. */
                    replace_code = REPL_OVERWRITE;
                    break;
                case 'V':
                    replace_code_all = REPL_NEW_VERSION;
                case 'v':
                    /* Create a new version of this file. */
                    replace_code = REPL_NEW_VERSION;
                    break;
                default:
                    /* Invalid response.  Try again. */
                    Info(slide, 1, ((char *)slide, InvalidResponse, answ));
            }
        } while (replace_code < 0);
    }
    return replace_code;
}



#define W(p)    (*(unsigned short*)(p))
#define L(p)    (*(unsigned long*)(p))
#define EQL_L(a, b)     ( L(a) == L(b) )
#define EQL_W(a, b)     ( W(a) == W(b) )

/*
 * Function find_vms_attrs() scans the ZIP entry extra field, if any,
 * and looks for VMS attribute records.  Various date-time attributes
 * are ignored if set_date_time is FALSE (typically for a directory).
 *
 * For a set of IZ records, a FAB and various XABs are created and
 * chained together.
 *
 * For a PK record, the pka_atr[] attribute descriptor array is
 * populated.
 *
 * The return value is a VAT_* value, according to the type of extra
 * field attribute data found.
 */
static int find_vms_attrs(__GPRO__ int set_date_time)
{
    uch *scan = G.extra_field;
    struct  EB_header *hdr;
    int len;
    int type=VAT_NONE;

    outfab = NULL;
    xabfhc = NULL;
    xabdat = NULL;
    xabrdt = NULL;
    xabpro = NULL;
    first_xab = last_xab = NULL;

    if (scan == NULL)
        return VAT_NONE;
    len = G.lrec.extra_field_length;

#define LINK(p) {/* Link xaballs and xabkeys into chain */      \
                if ( first_xab == NULL )                \
                        first_xab = (void *) p;         \
                if ( last_xab != NULL )                 \
                        last_xab->xab$l_nxt = (void *) p;       \
                last_xab = (void *) p;                  \
                p->xab$l_nxt = NULL;                    \
        }
    /* End of macro LINK */

    while (len > 0)
    {
        hdr = (struct EB_header *)scan;
        if (EQL_W(&hdr->tag, IZ_SIGNATURE))
        {
            /*
             *  Info-ZIP-style extra block decoding.
             */
            uch *blk;
            unsigned siz;
            uch *block_id;

            type = VAT_IZ;

            siz = hdr->size;
            blk = (uch *)(&hdr->data[0]);
            block_id = (uch *)(&((struct IZ_block *)hdr)->bid);

            if (EQL_L(block_id, FABSIG)) {
                outfab = (struct FAB *)extract_izvms_block(__G__ blk,
                  siz, NULL, (uch *)&cc$rms_fab, FABL);
            } else if (EQL_L(block_id, XALLSIG)) {
                xaball = (struct XABALL *)extract_izvms_block(__G__ blk,
                  siz, NULL, (uch *)&cc$rms_xaball, XALLL);
                LINK(xaball);
            } else if (EQL_L(block_id, XKEYSIG)) {
                xabkey = (struct XABKEY *)extract_izvms_block(__G__ blk,
                  siz, NULL, (uch *)&cc$rms_xabkey, XKEYL);
                LINK(xabkey);
            } else if (EQL_L(block_id, XFHCSIG)) {
                xabfhc = (struct XABFHC *) extract_izvms_block(__G__ blk,
                  siz, NULL, (uch *)&cc$rms_xabfhc, XFHCL);
            } else if (EQL_L(block_id, XDATSIG)) {
                if (set_date_time) {
                    xabdat = (struct XABDAT *) extract_izvms_block(__G__ blk,
                      siz, NULL, (uch *)&cc$rms_xabdat, XDATL);
                }
            } else if (EQL_L(block_id, XRDTSIG)) {
                if (set_date_time) {
                    xabrdt = (struct XABRDT *) extract_izvms_block(__G__ blk,
                      siz, NULL, (uch *)&cc$rms_xabrdt, XRDTL);
                }
            } else if (EQL_L(block_id, XPROSIG)) {
                xabpro = (struct XABPRO *) extract_izvms_block(__G__ blk,
                  siz, NULL, (uch *)&cc$rms_xabpro, XPROL);
            } else if (EQL_L(block_id, VERSIG)) {
#ifdef CHECK_VERSIONS
                char verbuf[80];
                unsigned verlen = 0;
                uch *vers;
                char *m;

                get_vms_version(verbuf, sizeof(verbuf));
                vers = extract_izvms_block(__G__ blk, siz,
                                           &verlen, NULL, 0);
                if ((m = strrchr((char *) vers, '-')) != NULL)
                    *m = '\0';  /* Cut out release number */
                if (strcmp(verbuf, (char *) vers) && uO.qflag < 2)
                {
                    Info(slide, 0, ((char *)slide,
                         "[ Warning: VMS version mismatch."));

                    Info(slide, 0, ((char *)slide,
                         "   This version %s --", verbuf));
                    strncpy(verbuf, (char *) vers, verlen);
                    verbuf[verlen] = '\0';
                    Info(slide, 0, ((char *)slide,
                         " version made by %s ]\n", verbuf));
                }
                free(vers);
#endif /* CHECK_VERSIONS */
            } else {
                Info(slide, 1, ((char *)slide,
                     "[ Warning: Unknown block signature %s ]\n",
                     block_id));
            }
        }
        else if (hdr->tag == PK_SIGNATURE)
        {
            /*
             *  PKWARE-style extra block decoding.
             */
            struct  PK_header   *blk;
            register byte   *scn;
            register int    len;

            type = VAT_PK;

            blk = (struct PK_header *)hdr;
            len = blk->size - (PK_HEADER_SIZE - EB_HEADSIZE);
            scn = (byte *)(&blk->data);
            pka_idx = 0;

            if (blk->crc32 != crc32(CRCVAL_INITIAL, scn, (extent)len))
            {
                Info(slide, 1, ((char *)slide,
                  "[ Warning: CRC error, discarding PKWARE extra field ]\n"));
                len = 0;
                type = VAT_NONE;
            }

            while (len > PK_FLDHDR_SIZE)
            {
                register struct  PK_field  *fld;
                int skip=0;

                fld = (struct PK_field *)scn;
                switch(fld->tag)
                {
                    case ATR$C_UCHAR:
                        pka_uchar = L(&fld->value);
                        break;
                    case ATR$C_RECATTR:
                        pka_rattr = *(struct fatdef *)(&fld->value);
                        break;
                    case ATR$C_UIC:
                    case ATR$C_ADDACLENT:
                        skip = !uO.X_flag;
                        break;
                    case ATR$C_CREDATE:
                    case ATR$C_REVDATE:
                    case ATR$C_EXPDATE:
                    case ATR$C_BAKDATE:
                    case ATR$C_ASCDATES:
                        skip = (set_date_time == FALSE);
                        break;
                }

                if ( !skip )
                {
                    pka_atr[pka_idx].atr$w_size = fld->size;
                    pka_atr[pka_idx].atr$w_type = fld->tag;
                    pka_atr[pka_idx].atr$l_addr = GVTC &fld->value;
                    ++pka_idx;
                }
                len -= fld->size + PK_FLDHDR_SIZE;
                scn += fld->size + PK_FLDHDR_SIZE;
            }
            pka_atr[pka_idx].atr$w_size = 0;    /* End of list */
            pka_atr[pka_idx].atr$w_type = 0;
            pka_atr[pka_idx].atr$l_addr = 0; /* NULL when DECC VAX gets fixed */
        }
        len -= hdr->size + EB_HEADSIZE;
        scan += hdr->size + EB_HEADSIZE;
    }

    if ( type == VAT_IZ )
    {
        if (outfab != NULL)
        {
            /* Do not link XABPRO or XABRDT now.
             * Leave them for sys$close() resp. set_direc_attribs().
             */
            outfab->fab$l_xab = NULL;
            if (xabfhc != NULL)
            {
                xabfhc->xab$l_nxt = outfab->fab$l_xab;
                outfab->fab$l_xab = (void *) xabfhc;
            }
            if (xabdat != NULL)
            {
                xabdat->xab$l_nxt = outfab->fab$l_xab;
                outfab->fab$l_xab = (void *) xabdat;
            }
            if (first_xab != NULL)      /* Link xaball,xabkey subchain */
            {
                last_xab->xab$l_nxt = outfab->fab$l_xab;
                outfab->fab$l_xab = (void *) first_xab;
            }
        }
        else
            type = VAT_NONE;
    }
    return type;
}



static void free_up()
{
    /*
     * Free up all allocated XABs.
     */
    if (xabdat != NULL) free(xabdat);
    if (xabpro != NULL) free(xabpro);
    if (xabrdt != NULL) free(xabrdt);
    if (xabfhc != NULL) free(xabfhc);
    while (first_xab != NULL)
    {
        struct XAB *x;

        x = (struct XAB *) first_xab->xab$l_nxt;
        free(first_xab);
        first_xab = x;
    }
    /* Free FAB storage, if not the static one. */
    if (outfab != NULL && outfab != &fileblk)
        free(outfab);
}



#ifdef CHECK_VERSIONS

static int get_vms_version(verbuf, len)
    char *verbuf;
    int len;
{
    int i = SYI$_VERSION;
    int verlen = 0;
    struct dsc$descriptor version;
    char *m;

    version.dsc$a_pointer = verbuf;
    version.dsc$w_length  = len - 1;
    version.dsc$b_dtype   = DSC$K_DTYPE_B;
    version.dsc$b_class   = DSC$K_CLASS_S;

    if (ERR(lib$getsyi(&i, 0, &version, &verlen, 0, 0)) || verlen == 0)
        return 0;

    /* Cut out trailing spaces "V5.4-3   " -> "V5.4-3" */
    for (m = verbuf + verlen, i = verlen - 1; i > 0 && verbuf[i] == ' '; --i)
        --m;
    *m = '\0';

    /* Cut out release number "V5.4-3" -> "V5.4" */
    if ((m = strrchr(verbuf, '-')) != NULL)
        *m = '\0';
    return strlen(verbuf) + 1;  /* Transmit ending '\0' too */
}

#endif /* CHECK_VERSIONS */



/* flush contents of output buffer */
int flush(__G__ rawbuf, size, unshrink)    /* return PK-type error code */
    __GDEF
    uch *rawbuf;
    ulg size;
    int unshrink;
{
    G.crc32val = crc32(G.crc32val, rawbuf, (extent)size);
    if (uO.tflag)
        return PK_COOL; /* Do not output. Update CRC only */
    else
        return (*_flush_routine)(__G__ rawbuf, size, 0);
}



static int _flush_blocks(__G__ rawbuf, size, final_flag)
                                                /* Asynchronous version */
    __GDEF
    uch *rawbuf;
    unsigned size;
    int final_flag;   /* 1 if this is the final flushout */
{
    int status;
    unsigned off = 0;

    while (size > 0)
    {
        if (curbuf->bufcnt < BUFS512)
        {
            unsigned ncpy;

            ncpy = size > (BUFS512 - curbuf->bufcnt) ?
                   (BUFS512 - curbuf->bufcnt) : size;
            memcpy(curbuf->buf + curbuf->bufcnt, rawbuf + off, ncpy);
            size -= ncpy;
            curbuf->bufcnt += ncpy;
            off += ncpy;
        }
        if (curbuf->bufcnt == BUFS512)
        {
            status = WriteBuffer(__G__ curbuf->buf, curbuf->bufcnt);
            if (status)
                return status;
            curbuf = curbuf->next;
            curbuf->bufcnt = 0;
        }
    }

    return (final_flag && (curbuf->bufcnt > 0)) ?
        WriteBuffer(__G__ curbuf->buf, curbuf->bufcnt) :
        PK_COOL;
}



#ifdef ASYNCH_QIO
static int WriteQIO(__G__ buf, len)
    __GDEF
    uch *buf;
    unsigned len;
{
    int status;

    if (pka_io_pending) {
        status = sys$synch(0, &pka_io_iosb);
        if (!ERR(status))
            status = pka_io_iosb.status;
        if (ERR(status))
        {
            vms_msg(__G__ "[ WriteQIO: sys$synch found I/O failure ]\n",
                    status);
            return PK_DISK;
        }
        pka_io_pending = FALSE;
    }
    /*
     *   Put content of buffer as a single VB
     */
    status = sys$qio(0, pka_devchn, IO$_WRITEVBLK,
                     &pka_io_iosb, 0, 0,
                     buf, len, pka_vbn,
                     0, 0, 0);
    if (ERR(status))
    {
        vms_msg(__G__ "[ WriteQIO: sys$qio failed ]\n", status);
        return PK_DISK;
    }
    pka_io_pending = TRUE;
    pka_vbn += (len>>9);

    return PK_COOL;
}

/*
   2004-10-01 SMS.  Changed to clear the extra byte written out by qio()
   and sys$write() when an odd byte count is incremented to the next
   even value, either explicitly (qio), or implicitly (sys$write), on
   the theory that a reliable NUL beats left-over garbage.  Alpha and
   VAX object files seem frequently to have even more than one byte of
   extra junk past EOF, so this may not help them.
*/

static int _flush_qio(__G__ rawbuf, size, final_flag)
                                                /* Asynchronous version */
    __GDEF
    uch *rawbuf;
    unsigned size;
    int final_flag;   /* 1 if this is the final flushout */
{
    int status;
    unsigned off = 0;

    while (size > 0)
    {
        if (curbuf->bufcnt < BUFS512)
        {
            unsigned ncpy;

            ncpy = size > (BUFS512 - curbuf->bufcnt) ?
                   (BUFS512 - curbuf->bufcnt) : size;
            memcpy(curbuf->buf + curbuf->bufcnt, rawbuf + off, ncpy);
            size -= ncpy;
            curbuf->bufcnt += ncpy;
            off += ncpy;
        }
        if (curbuf->bufcnt == BUFS512)
        {
            status = WriteQIO(__G__ curbuf->buf, curbuf->bufcnt);
            if (status)
                return status;
            curbuf = curbuf->next;
            curbuf->bufcnt = 0;
        }
    }

    if (final_flag && (curbuf->bufcnt > 0))
    {
        unsigned bufcnt_even;

        /* Round up to an even byte count. */
        bufcnt_even = (curbuf->bufcnt+1) & (~1);
        /* If there is one, clear the extra byte. */
        if (bufcnt_even > curbuf->bufcnt)
            curbuf->buf[curbuf->bufcnt] = '\0';

        return WriteQIO(curbuf->buf, bufcnt_even);
    }
    else
    {
        return PK_COOL;
    }
}

#else /* !ASYNCH_QIO */

static int _flush_qio(__G__ rawbuf, size, final_flag)
    __GDEF
    uch *rawbuf;
    unsigned size;
    int final_flag;   /* 1 if this is the final flushout */
{
    int status;
    uch *out_ptr=rawbuf;

    if ( final_flag )
    {
        if ( loccnt > 0 )
        {
            unsigned loccnt_even;

            /* Round up to an even byte count. */
            loccnt_even = (loccnt+1) & (~1);
            /* If there is one, clear the extra byte. */
            if (loccnt_even > loccnt)
                locbuf[loccnt] = '\0';

            status = sys$qiow(0, pka_devchn, IO$_WRITEVBLK,
                              &pka_io_iosb, 0, 0,
                              locbuf,
                              loccnt_even,
                              pka_vbn,
                              0, 0, 0);
            if (!ERR(status))
                status = pka_io_iosb.status;
            if (ERR(status))
            {
                vms_msg(__G__ "[ Write QIO failed ]\n", status);
                return PK_DISK;
            }
        }
        return PK_COOL;
    }

    if ( loccnt > 0 )
    {
        /*
         *   Fill local buffer upto 512 bytes then put it out
         */
        unsigned ncpy;

        ncpy = 512-loccnt;
        if ( ncpy > size )
            ncpy = size;

        memcpy(locptr, out_ptr, ncpy);
        locptr += ncpy;
        loccnt += ncpy;
        size -= ncpy;
        out_ptr += ncpy;
        if ( loccnt == 512 )
        {
            status = sys$qiow(0, pka_devchn, IO$_WRITEVBLK,
                              &pka_io_iosb, 0, 0,
                              locbuf, loccnt, pka_vbn,
                              0, 0, 0);
            if (!ERR(status))
                status = pka_io_iosb.status;
            if (ERR(status))
            {
                vms_msg(__G__ "[ Write QIO failed ]\n", status);
                return PK_DISK;
            }

            pka_vbn++;
            loccnt = 0;
            locptr = locbuf;
        }
    }

    if ( size >= 512 )
    {
        unsigned nblk, put_cnt;

        /*
         *   Put rest of buffer as a single VB
         */
        put_cnt = (nblk = size>>9)<<9;
        status = sys$qiow(0, pka_devchn, IO$_WRITEVBLK,
                          &pka_io_iosb, 0, 0,
                          out_ptr, put_cnt, pka_vbn,
                          0, 0, 0);
        if (!ERR(status))
            status = pka_io_iosb.status;
        if (ERR(status))
        {
            vms_msg(__G__ "[ Write QIO failed ]\n", status);
            return PK_DISK;
        }

        pka_vbn += nblk;
        out_ptr += put_cnt;
        size -= put_cnt;
    }

    if ( size > 0 )
    {
        memcpy(locptr, out_ptr, size);
        loccnt += size;
        locptr += size;
    }

    return PK_COOL;
}
#endif /* ?ASYNCH_QIO */



/*
 * The routine _flush_varlen() requires: "(size & 1) == 0"
 * (The variable-length record algorithm assumes an even byte-count!)
 */
static int _flush_varlen(__G__ rawbuf, size, final_flag)
    __GDEF
    uch *rawbuf;
    unsigned size;
    int final_flag;
{
    unsigned nneed;
    unsigned reclen;
    uch *inptr=rawbuf;

    /*
     * Flush local buffer
     */

    if ( loccnt > 0 )           /* incomplete record left from previous call */
    {
        reclen = *(ush*)locbuf;
        nneed = reclen + 2 - loccnt;
        if ( nneed > size )
        {
            if ( size+loccnt > BUFSMAXREC )
            {
                char buf[80];
                Info(buf, 1, (buf,
                     "[ Record too long (%u bytes) ]\n", reclen));
                return PK_DISK;
            }
            memcpy(locbuf+loccnt, inptr, size);
            loccnt += size;
            size = 0;
        }
        else
        {
            memcpy(locbuf+loccnt, inptr, nneed);
            loccnt += nneed;
            size -= nneed;
            inptr += nneed;
            if ( reclen & 1 )
            {
                size--;
                inptr++;
            }
            if ( WriteRecord(__G__ locbuf+2, reclen) )
                return PK_DISK;
            loccnt = 0;
        }
    }
    /*
     * Flush incoming records
     */
    while (size > 0)
    {
        reclen = *(ush*)inptr;
        if ( reclen+2 <= size )
        {
            if (WriteRecord(__G__ inptr+2, reclen))
                return PK_DISK;
            size -= 2+reclen;
            inptr += 2+reclen;
            if ( reclen & 1 )
            {
                --size;
                ++inptr;
            }
        }
        else
        {
            memcpy(locbuf, inptr, size);
            loccnt = size;
            size = 0;
        }

    }
    /*
     * Final flush rest of local buffer
     */
    if ( final_flag && loccnt > 0 )
    {
        char buf[80];

        Info(buf, 1, (buf,
             "[ Warning, incomplete record of length %u ]\n",
             (unsigned)*(ush*)locbuf));
        if ( WriteRecord(__G__ locbuf+2, loccnt-2) )
            return PK_DISK;
    }
    return PK_COOL;
}



/*
 *   Routine _flush_stream breaks decompressed stream into records
 *   depending on format of the stream (fab->rfm, G.pInfo->textmode, etc.)
 *   and puts out these records. It also handles CR LF sequences.
 *   Should be used when extracting *text* files.
 */

#define VT      0x0B
#define FF      0x0C

/* The file is from MSDOS/OS2/NT -> handle CRLF as record end, throw out ^Z */

/* GRR NOTES:  cannot depend on hostnum!  May have "flip'd" file or re-zipped
 * a Unix file, etc. */

#ifdef USE_ORIG_DOS
# define ORG_DOS \
          (G.pInfo->hostnum==FS_FAT_    \
        || G.pInfo->hostnum==FS_HPFS_   \
        || G.pInfo->hostnum==FS_NTFS_)
#else
# define ORG_DOS    1
#endif

/* Record delimiters */
#ifdef undef
#define RECORD_END(c, f)                                                \
(    ( ORG_DOS || G.pInfo->textmode ) && c==CTRLZ                       \
  || ( f == FAB$C_STMLF && c==LF )                                      \
  || ( f == FAB$C_STMCR || ORG_DOS || G.pInfo->textmode ) && c==CR      \
  || ( f == FAB$C_STM && (c==CR || c==LF || c==FF || c==VT) )           \
)
#else
#   define  RECORD_END(c, f)   ((c) == LF || (c) == (CR))
#endif

static unsigned find_eol(p, n, l)
/*
 *  Find first CR, LF, CR/LF or LF/CR in string 'p' of length 'n'.
 *  Return offset of the sequence found or 'n' if not found.
 *  If found, return in '*l' length of the sequence (1 or 2) or
 *  zero if sequence end not seen, i.e. CR or LF is last char
 *  in the buffer.
 */
    ZCONST uch *p;
    unsigned n;
    unsigned *l;
{
    unsigned off = n;
    ZCONST uch *q;

    *l = 0;

    for (q=p ; n > 0 ; --n, ++q)
        if ( RECORD_END(*q, rfm) )
        {
            off = q-p;
            break;
        }

    if ( n > 1 )
    {
        *l = 1;
        if ( ( q[0] == CR && q[1] == LF ) || ( q[0] == LF && q[1] == CR ) )
            *l = 2;
    }

    return off;
}

/* Record delimiters that must be put out */
#define PRINT_SPEC(c)   ( (c)==FF || (c)==VT )



static int _flush_stream(__G__ rawbuf, size, final_flag)
    __GDEF
    uch *rawbuf;
    unsigned size;
    int final_flag; /* 1 if this is the final flushout */
{
    int rest;
    unsigned end = 0, start = 0;

    if (size == 0 && loccnt == 0)
        return PK_COOL;         /* Nothing to do ... */

    if ( final_flag )
    {
        unsigned recsize;

        /*
         * This is flush only call. size must be zero now.
         * Just eject everything we have in locbuf.
         */
        recsize = loccnt - (got_eol ? 1 : 0);
        /*
         *  If the last char of file was ^Z ( end-of-file in MSDOS ),
         *  we will see it now.
         */
        if ( recsize==1 && locbuf[0] == CTRLZ )
            return PK_COOL;

        return WriteRecord(__G__ locbuf, recsize);
    }


    if ( loccnt > 0 )
    {
        /* Find end of record partially saved in locbuf */

        unsigned recsize;
        int complete=0;

        if ( got_eol )
        {
            recsize = loccnt - 1;
            complete = 1;

            if ( (got_eol == CR && rawbuf[0] == LF) ||
                 (got_eol == LF && rawbuf[0] == CR) )
                end = 1;

            got_eol = 0;
        }
        else
        {
            unsigned eol_len;
            unsigned eol_off;

            eol_off = find_eol(rawbuf, size, &eol_len);

            if ( loccnt+eol_off > BUFSMAXREC )
            {
                /*
                 *  No room in locbuf. Dump it and clear
                 */
                char buf[80];           /* CANNOT use slide for Info() */

                recsize = loccnt;
                start = 0;
                Info(buf, 1, (buf,
                     "[ Warning: Record too long (%u) ]\n", loccnt+eol_off));
                complete = 1;
                end = 0;
            }
            else
            {
                if ( eol_off >= size )
                {
                    end = size;
                    complete = 0;
                }
                else if ( eol_len == 0 )
                {
                    got_eol = rawbuf[eol_off];
                    end = size;
                    complete = 0;
                }
                else
                {
                    memcpy(locptr, rawbuf, eol_off);
                    recsize = loccnt + eol_off;
                    locptr += eol_off;
                    loccnt += eol_off;
                    end = eol_off + eol_len;
                    complete = 1;
                }
            }
        }

        if ( complete )
        {
            if (WriteRecord(__G__ locbuf, recsize))
                return PK_DISK;
            loccnt = 0;
            locptr = locbuf;
        }
    }                           /* end if ( loccnt ) */

    for (start = end; start < size && end < size; )
    {
        unsigned eol_off, eol_len;

        got_eol = 0;

#ifdef undef
        if (uO.cflag)
            /* skip CR's at the beginning of record */
            while (start < size && rawbuf[start] == CR)
                ++start;
#endif

        if ( start >= size )
            continue;

        /* Find record end */
        end = start+(eol_off = find_eol(rawbuf+start, size-start, &eol_len));

        if ( end >= size )
            continue;

        if ( eol_len > 0 )
        {
            if ( WriteRecord(__G__ rawbuf+start, end-start) )
                return PK_DISK;
            start = end + eol_len;
        }
        else
        {
            got_eol = rawbuf[end];
            end = size;
            continue;
        }
    }

    rest = size - start;

    if (rest > 0)
    {
        if ( rest > BUFSMAXREC )
        {
            unsigned recsize;
            char buf[80];               /* CANNOT use slide for Info() */

            recsize = rest - (got_eol ? 1 : 0 );
            Info(buf, 1, (buf,
                 "[ Warning: Record too long (%u) ]\n", recsize));
            got_eol = 0;
            return WriteRecord(__G__ rawbuf+start, recsize);
        }
        else
        {
            memcpy(locptr, rawbuf + start, rest);
            locptr += rest;
            loccnt += rest;
        }
    }
    return PK_COOL;
}



static int WriteBuffer(__G__ buf, len)
    __GDEF
    uch *buf;
    unsigned len;
{
    int status;

    if (uO.cflag)
    {
        (void)(*G.message)((zvoid *)&G, buf, len, 0);
    }
    else
    {
        status = sys$wait(outrab);
        if (ERR(status))
        {
            vms_msg(__G__ "[ WriteBuffer: sys$wait failed ]\n", status);
            if (outrab->rab$l_stv != 0)
            {
                vms_msg(__G__ "", outrab->rab$l_stv);
            }
        }

        /* If odd byte count, then this must be the final record.
           Clear the extra byte past EOF to help keep the file clean.
        */
        if (len & 1)
            buf[len] = '\0';

        outrab->rab$w_rsz = len;
        outrab->rab$l_rbf = (char *) buf;

        if (ERR(status = sys$write(outrab)))
        {
            vms_msg(__G__ "[ WriteBuffer: sys$write failed ]\n", status);
            if (outrab->rab$l_stv != 0)
            {
                vms_msg(__G__ "", outrab->rab$l_stv);
            }
            return PK_DISK;
        }
    }
    return PK_COOL;
}



static int WriteRecord(__G__ rec, len)
    __GDEF
    uch *rec;
    unsigned len;
{
    int status;

    if (uO.cflag)
    {
        (void)(*G.message)((zvoid *)&G, rec, len, 0);
        (void)(*G.message)((zvoid *)&G, (uch *) ("\n"), 1, 0);
    }
    else
    {
        if (ERR(status = sys$wait(outrab)))
        {
            vms_msg(__G__ "[ WriteRecord: sys$wait failed ]\n", status);
            if (outrab->rab$l_stv != 0)
            {
                vms_msg(__G__ "", outrab->rab$l_stv);
            }
        }
        outrab->rab$w_rsz = len;
        outrab->rab$l_rbf = (char *) rec;

        if (ERR(status = sys$put(outrab)))
        {
            vms_msg(__G__ "[ WriteRecord: sys$put failed ]\n", status);
            if (outrab->rab$l_stv != 0)
            {
                vms_msg(__G__ "", outrab->rab$l_stv);
            }
            return PK_DISK;
        }
    }
    return PK_COOL;
}



#ifdef SYMLINKS
/* Read symlink text from a still-open rms file. */

static int _read_link_rms(int byte_count, char *link_text_buf)
{
    /* Use RMS to read the link text into the user's buffer.
     * Rewind, then read byte count = byte_count.
     * NUL-terminate the link text.
     *
     * $WAIT may be pointless if not async, but $WAIT elsewhere seems
     * to be used unconditionally, so what do I know?
     */
    int sts;
    int bytes_read;

    /* Clear the bytes-read count. */
    bytes_read = 0;

    /* Wait for anything pending. */
    sts = sys$wait(outrab);
    {
        /* Rewind. */
        sts = sys$rewind(outrab);
        if (!ERR(sts))
        {
            /* Wait for $REWIND. */
            sts = sys$wait(outrab);
            if (!ERR(sts))
            {
                /* Read the link text. */
                outrab->rab$w_usz = byte_count;
                outrab->rab$l_ubf = link_text_buf;
                sts = sys$read(outrab);
                if (!ERR(sts))
                {
                    /* Wait for $READ. */
                    sts = sys$wait(outrab);

                    if (!ERR(sts))
                        /* Set the resultant byte count. */
                        bytes_read = outrab->rab$w_rsz;
                }
            }
        }
    }

    /* NUL-terminate the link text. */
    link_text_buf[bytes_read] = '\0';

    return sts;
}

#endif /* SYMLINKS */



void close_outfile(__G)
    __GDEF
{
    int status;

    status = (*_flush_routine)(__G__ NULL, 0, 1);
    if (status)
        return /* PK_DISK */;
    if (uO.cflag)
        return /* PK_COOL */;   /* Don't close stdout */
    /* return */ (*_close_routine)(__G);
}



static int _close_rms(__GPRO)
{
    int status;
    struct XABPRO pro;
    int retcode = PK_OK;

#ifdef SYMLINKS

/*----------------------------------------------------------------------
    UNIX description:
    If symbolic links are supported, allocate storage for a symlink
    control structure, put the uncompressed "data" and other required
    info in it, and add the structure to the "deferred symlinks" chain.
    Since we know it's a symbolic link to start with, we shouldn't have
    to worry about overflowing unsigned ints with unsigned longs.
----------------------------------------------------------------------*/

    if (G.symlnk) {
        extent ucsize = (extent)G.lrec.ucsize;

        /* 2007-03-03 SMS.
         * If the symlink is already a symlink (restored with VMS/RMS
         * symlink attributes), then read the link text from the file,
         * and close the file (using the appropriate methods), and then
         * return.
         */
        if (G.pInfo->symlink == 0)
        {
            if (QCOND2)
            {
                /* Link text storage. */
                char* link_target = malloc(ucsize + 1);

                if (link_target == NULL)
                {
                    Info(slide, 0x201, ((char *)slide,
                      "warning:  cannot show symlink (%s) target, no mem\n",
                      FnFilter1(G.filename)));
                      retcode = PK_MEM;
                }
                else
                {
                    /* Read the link text. */
                    status = _read_link_rms(ucsize, link_target);

                    if (ERR(status))
                    {
                        Info(slide, 0x201, ((char *)slide,
                          "warning:  error reading symlink text: %s\n",
                          strerror(EVMSERR, status)));
                        retcode = PK_DISK;
                    }
                    else
                    {
                        Info(slide, 0, ((char *)slide, "-> %s ",
                          FnFilter1(link_target)));
                    }

                    free(link_target);
                }
            }
        }
        else
        {
            extent slnk_entrysize;
            slinkentry *slnk_entry;

            /* It's a symlink in need of post-processing. */
            /* Size of the symlink entry is the sum of
             *  (struct size (includes 1st '\0') + 1 additional trailing '\0'),
             *  system specific attribute data size (might be 0),
             *  and the lengths of name and link target.
             */
            slnk_entrysize = (sizeof(slinkentry) + 1) +
                             ucsize + strlen(G.filename);

            if (slnk_entrysize < ucsize) {
                Info(slide, 0x201, ((char *)slide,
                  "warning:  symbolic link (%s) failed: mem alloc overflow\n",
                  FnFilter1(G.filename)));
                retcode = PK_ERR;
            }
            else
            {
                if ((slnk_entry = (slinkentry *)malloc(slnk_entrysize))
                    == NULL) {
                    Info(slide, 0x201, ((char *)slide,
                      "warning:  symbolic link (%s) failed, no mem\n",
                      FnFilter1(G.filename)));
                    retcode = PK_MEM;
                }
                else
                {
                    slnk_entry->next = NULL;
                    slnk_entry->targetlen = ucsize;
                    /* don't set attributes for symlinks */
                    slnk_entry->attriblen = 0;
                    slnk_entry->target = slnk_entry->buf;
                    slnk_entry->fname = slnk_entry->target + ucsize + 1;
                    strcpy(slnk_entry->fname, G.filename);

                    /* Read the link text using the appropriate method. */
                    status = _read_link_rms(ucsize, slnk_entry->target);

                    if (ERR(status))
                    {
                        Info(slide, 0x201, ((char *)slide,
                          "warning:  error reading symlink text (rms): %s\n",
                          strerror(EVMSERR, status)));
                        free(slnk_entry);
                        retcode = PK_DISK;
                    }
                    else
                    {
                        if (QCOND2)
                            Info(slide, 0, ((char *)slide, "-> %s ",
                              FnFilter1(slnk_entry->target)));

                        /* Add this symlink record to the list of
                           deferred symlinks. */
                        if (G.slink_last != NULL)
                            G.slink_last->next = slnk_entry;
                        else
                            G.slink_head = slnk_entry;
                        G.slink_last = slnk_entry;
                    }
                }
            }
        }
    }
#endif /* SYMLINKS */

    /* Link XABRDT, XABDAT, and (optionally) XABPRO. */
    if (xabrdt != NULL)
    {
        xabrdt->xab$l_nxt = NULL;
        outfab->fab$l_xab = (void *) xabrdt;
    }
    else
    {
        rdt.xab$l_nxt = NULL;
        outfab->fab$l_xab = (void *) &rdt;
    }
    if (xabdat != NULL)
    {
        xabdat->xab$l_nxt = outfab->fab$l_xab;
        outfab->fab$l_xab = (void *)xabdat;
    }

    if (xabpro != NULL)
    {
        if ( !uO.X_flag )
            xabpro->xab$l_uic = 0;    /* Use default (user's) uic */
        xabpro->xab$l_nxt = outfab->fab$l_xab;
        outfab->fab$l_xab = (void *) xabpro;
    }
    else
    {
        pro = cc$rms_xabpro;
        pro.xab$w_pro = G.pInfo->file_attr;
        pro.xab$l_nxt = outfab->fab$l_xab;
        outfab->fab$l_xab = (void *) &pro;
    }

    status = sys$wait(outrab);
    if (ERR(status))
    {
        vms_msg(__G__ "[ _close_rms: sys$wait failed ]\n", status);
        if (outrab->rab$l_stv != 0)
        {
            vms_msg(__G__ "", outrab->rab$l_stv);
        }
    }

    status = sys$close(outfab);
#ifdef DEBUG
    if (ERR(status))
    {
        vms_msg(__G__
          "\r[ Warning: cannot set owner/protection/time attributes ]\n",
          status);
        if (outfab->fab$l_stv != 0)
        {
            vms_msg(__G__ "", outfab->fab$l_stv);
        }
        retcode = PK_WARN;
    }
#endif
    free_up();
    return retcode;
}



static int _close_qio(__GPRO)
{
    int status;

    pka_fib.FIB$L_ACCTL =
        FIB$M_WRITE | FIB$M_NOTRUNC ;
    pka_fib.FIB$W_EXCTL = 0;

    pka_fib.FIB$W_FID[0] =
    pka_fib.FIB$W_FID[1] =
    pka_fib.FIB$W_FID[2] =
    pka_fib.FIB$W_DID[0] =
    pka_fib.FIB$W_DID[1] =
    pka_fib.FIB$W_DID[2] = 0;

#ifdef ASYNCH_QIO
    if (pka_io_pending) {
        status = sys$synch(0, &pka_io_iosb);
        if (!ERR(status))
            status = pka_io_iosb.status;
        if (ERR(status))
        {
            vms_msg(__G__ "[ _close_qio: sys$synch found I/O failure ]\n",
                    status);
        }
        pka_io_pending = FALSE;
    }
#endif /* ASYNCH_QIO */

#ifdef SYMLINKS
    if (G.symlnk && QCOND2)
    {
        /* Read back the symlink target specification for display purpose. */
        extent ucsize = (extent)G.lrec.ucsize;
        char *link_target;   /* Link text storage. */

        if ((link_target = malloc(ucsize + 1)) == NULL)
        {
            Info(slide, 0x201, ((char *)slide,
              "warning:  cannot show symlink (%s) target, no mem\n",
              FnFilter1(G.filename)));
        }
        else
        {
            unsigned bytes_read = 0;

            status = sys$qiow(0,                /* event flag */
                              pka_devchn,       /* channel */
                              IO$_READVBLK,     /* function */
                              &pka_io_iosb,     /* IOSB */
                              0,                /* AST address */
                              0,                /* AST parameter */
                              link_target,      /* P1 = buffer address */
                              ucsize,           /* P2 = requested byte count */
                              1,                /* P3 = VBN (1 = first) */
                              0,                /* P4 (not used) */
                              0,                /* P5 (not used) */
                              0);               /* P6 (not used) */

            if (!ERR(status))
                /* Final status. */
                status = pka_io_iosb.status;

            /* Set the resultant byte count. */
            if (!ERR(status))
                bytes_read = pka_io_iosb.count;

            /* NUL-terminate the link text. */
            link_target[bytes_read] = '\0';

            if (ERR(status))
            {
                Info(slide, 0x201, ((char *)slide,
                  "warning:  error reading symlink text (qio): %s\n",
                  strerror(EVMSERR, status)));
            }
            else
            {
                Info(slide, 0, ((char *)slide, "-> %s ",
                  FnFilter1(link_target)));
            }

            free(link_target);

        }
    }
#endif /* SYMLINKS */

    status = sys$qiow(0, pka_devchn, IO$_DEACCESS, &pka_acp_iosb,
                      0, 0,
                      &pka_fibdsc, 0, 0, 0,
                      pka_atr, 0);

    sys$dassgn(pka_devchn);
    if ( !ERR(status) )
        status = pka_acp_iosb.status;
    if ( ERR(status) )
    {
        vms_msg(__G__ "[ Deaccess QIO failed ]\n", status);
        return PK_DISK;
    }
    return PK_COOL;
}



#ifdef SET_DIR_ATTRIB

/*
 * 2006-10-04 SMS.
 * vms_path_fixdown().
 *
 * Convert VMS directory spec to VMS directory file name.  That is,
 * change "dev:[a.b.c.e]" to "dev:[a.b.c]e.DIR;1".  The result (always
 * larger than the source) is returned in the user's buffer.
 */

#define DIR_TYPE_VER ".DIR;1"

static char *vms_path_fixdown(ZCONST char *dir_spec, char *dir_file)
{
    char dir_close;
    char dir_open;
    unsigned i;
    unsigned dir_spec_len;

    dir_spec_len = strlen(dir_spec);
    if (dir_spec_len == 0) return NULL;
    i = dir_spec_len - 1;
    dir_close = dir_spec[i];

    /* Identify the directory delimiters (which must exist). */
    if (dir_close == ']')
    {
        dir_open = '[';
    }
    else if (dir_close == '>')
    {
        dir_open = '<';
    }
    else
    {
        return NULL;
    }

    /* Find the beginning of the last directory name segment. */
    while ((i > 0) && ((dir_spec[i - 1] == '^') ||
           ((dir_spec[i] != '.') && (dir_spec[i] != dir_open))))
    {
        i--;
    }

    /* Form the directory file name from the pieces. */
    if (dir_spec[i] == dir_open)
    {
        /* Top-level directory. */
        sprintf(dir_file, "%.*s000000%c%.*s%s",
          /*  "dev:[" "000000" "]" */
          (i + 1), dir_spec, dir_close,
          /*  "a" ".DIR;1" */
          (dir_spec_len - i - 2), (dir_spec + i + 1), DIR_TYPE_VER);
    }
    else
    {
        /* Non-top-level directory. */
        sprintf(dir_file, "%.*s%c%.*s%s",
          /*  "dev:[a.b.c" "]" */
          i, dir_spec, dir_close,
          /*  "e" ".DIR;1" */
          (dir_spec_len - i - 2), (dir_spec + i + 1), DIR_TYPE_VER);
    }
    return dir_file;
} /* end function vms_path_fixdown(). */



/* Save directory attributes (as the archive's extra field). */

/* 2006-12-13 SMS.
 * This could probably be made more efficient by analyzing the data
 * here, extracting the important data, and saving only what's needed.
 * Given the existing code, it seemed simpler to save them all here, and
 * deal with what's important in set_direc_attribs().
 */

int defer_dir_attribs(__G__ pd)
    __GDEF
    direntry **pd;
{
    vmsdirattr *d_entry;
    unsigned fnlen;
    unsigned xlen;

    /* Allocate space to save the file (directory) name, the extra
     * block, and all the other data needed by the extra-block data
     * scanner functions.  If that works, save the data.
     */
    fnlen = strlen(G.filename);
    xlen = G.lrec.extra_field_length;
    d_entry = (vmsdirattr *) malloc(sizeof(vmsdirattr) + fnlen + xlen);
    *pd = (direntry *) d_entry;
    if (d_entry == (vmsdirattr *) NULL)
    {
        return PK_MEM;
    }

    /* Save extra block length and data. */
    d_entry->xlen = xlen;
    memcpy(d_entry->buf, G.extra_field, xlen);

    /* Set pointer to file (directory) name. */
    d_entry->fn = d_entry->buf + xlen;

    /* Save file (directory) name. */
    strcpy(d_entry->fn, G.filename);
    /* Strip the closing ']' char, to allow proper sorting. */
    d_entry->fn[fnlen - 1] = '\0';

    /* Save generic permission data from mapattr(). */
    d_entry->perms = G.pInfo->file_attr;

    /* Save G.lrec.last_mod_dos_datetime. */
    d_entry->mod_dos_datetime = G.lrec.last_mod_dos_datetime;

    return PK_OK;
} /* end function defer_dir_attribs() */



int set_direc_attribs(__G__ d)
    __GDEF
    direntry *d;
{
    uch *sav_ef_ptr;
    int i;
    int status;
    int type;
    ush attr;
    struct XABPRO pro;
    char dir_name[NAM_MAXRSS + 1];
    char warnmsg[NAM_MAXRSS + 128]; /* Name length + message length. */
    int retcode = PK_OK;

    /* Re-append the closing ']' character which has been stripped in
     * defer_dir_attribs() for compatibility with generic sorting code.
     */
    strcat(VmsAtt(d)->fn, "]");

    /* Convert "[a.b.c]" form into "[a.b]c.DIR;1" */
    vms_path_fixdown(VmsAtt(d)->fn, dir_name);

    /* Dummy up critical global (G) data from the preserved directory
     * attribute data.
     */
    sav_ef_ptr = G.extra_field;
    G.extra_field = (uch *)((VmsAtt(d)->xlen > 0) ? VmsAtt(d)->buf : NULL);
    G.lrec.extra_field_length = VmsAtt(d)->xlen;

    /* Extract the VMS file attributes from the preserved attribute
     * data, if they exist, and restore the date-time stamps.
     */
    type = find_vms_attrs(__G__ (uO.D_flag <= 0));

    if (outfab == NULL)
    {
        /* Default and PK schemes need a FAB.  (IZ supplies one.)
         * In a degenerate case, this could be the first use of fileblk,
         * so we assume that we need to initialize it.
         */
        fileblk = cc$rms_fab;           /* Initialize FAB. */
        outfab = &fileblk;              /* Set pointer used elsewhere. */
    }

    /* Arrange FAB-NAM[L] for file (directory) access. */
    if (type != VAT_NONE)
    {
        if (type == VAT_IZ)
        {
            /* Make an attribute descriptor list for the VMS creation and
             * revision dates (which were stored in the IZ XABs by
             * find_vms_attrs()).
             */
            pka_idx = 0;

            if (xabrdt != NULL)
            {
                /* Revision date-time from XABRDT. */
                pka_atr[pka_idx].atr$w_size = 8;
                pka_atr[pka_idx].atr$w_type = ATR$C_REVDATE;
                pka_atr[pka_idx].atr$l_addr = GVTC &xabrdt->xab$q_rdt;
                ++pka_idx;
            }
            if (xabdat != NULL)
            {
                /* Trust the XABRDT value for revision date. */
                if (xabrdt == NULL)
                {
                    /* Revision date-time from XABDAT. */
                    pka_atr[pka_idx].atr$w_size = 8;
                    pka_atr[pka_idx].atr$w_type = ATR$C_REVDATE;
                    pka_atr[pka_idx].atr$l_addr = GVTC &xabdat->xab$q_rdt;
                    ++pka_idx;
                }
                /* Creation date-time from XABDAT. */
                pka_atr[pka_idx].atr$w_size = 8;
                pka_atr[pka_idx].atr$w_type = ATR$C_CREDATE;
                pka_atr[pka_idx].atr$l_addr = GVTC &xabdat->xab$q_cdt;
                ++pka_idx;
            }
            if (xabpro != NULL)
            {
                if ( uO.X_flag ) {
                    pka_atr[pka_idx].atr$w_size = 4;
                    pka_atr[pka_idx].atr$w_type = ATR$C_UIC;
                    pka_atr[pka_idx].atr$l_addr = GVTC &xabpro->xab$l_uic;
                    ++pka_idx;
                }
                attr = xabpro->xab$w_pro;
            }
            else
            {
                /* Revoke directory Delete permission for all. */
                attr = VmsAtt(d)->perms
                      | (((1<< XAB$V_NODEL)<< XAB$V_SYS)|
                         ((1<< XAB$V_NODEL)<< XAB$V_OWN)|
                         ((1<< XAB$V_NODEL)<< XAB$V_GRP)|
                         ((1<< XAB$V_NODEL)<< XAB$V_WLD));
            }
            pka_atr[pka_idx].atr$w_size = 2;
            pka_atr[pka_idx].atr$w_type = ATR$C_FPRO;
            pka_atr[pka_idx].atr$l_addr = GVTC &attr;
            ++pka_idx;
        }
    }
    else
    {
        /* No VMS attribute data were found.  Prepare to assemble
         * non-VMS attribute data.
         */
        pka_idx = 0;

        /* Get the (already converted) non-VMS permissions. */
        attr = VmsAtt(d)->perms;        /* Use right-sized prot storage. */

        /* Revoke directory Delete permission for all. */
        attr |= (((1<< XAB$V_NODEL)<< XAB$V_SYS)|
                 ((1<< XAB$V_NODEL)<< XAB$V_OWN)|
                 ((1<< XAB$V_NODEL)<< XAB$V_GRP)|
                 ((1<< XAB$V_NODEL)<< XAB$V_WLD));

        pka_atr[pka_idx].atr$w_size = 2;
        pka_atr[pka_idx].atr$w_type = ATR$C_FPRO;
        pka_atr[pka_idx].atr$l_addr = GVTC &attr;
        ++pka_idx;

        /* Restore directory date-time if user requests it (-D). */
        if (uO.D_flag <= 0)
        {
            /* Set the directory date-time from the non-VMS data.
             * Dummy up the DOS-style modification date into global (G)
             * data from the preserved directory attribute data.
             */
            G.lrec.last_mod_dos_datetime = VmsAtt(d)->mod_dos_datetime;

            /* Extract date-time data from the normal attribute data. */
            set_default_datetime_XABs(__G);

            /* Make an attribute descriptor list for the VMS creation
             * and revision dates (which were stored in the XABs by
             * set_default_datetime_XABs()).
             */
            pka_atr[pka_idx].atr$w_size = 8;
            pka_atr[pka_idx].atr$w_type = ATR$C_CREDATE;
            pka_atr[pka_idx].atr$l_addr = GVTC &dattim.xab$q_cdt;
            ++pka_idx;
            pka_atr[pka_idx].atr$w_size = 8;
            pka_atr[pka_idx].atr$w_type = ATR$C_REVDATE;
            pka_atr[pka_idx].atr$l_addr = GVTC &rdt.xab$q_rdt;
            ++pka_idx;
        }

        /* Set the directory protection from the non-VMS data. */

        /* Terminate the attribute descriptor list. */
        pka_atr[pka_idx].atr$w_size = 0;    /* End of list */
        pka_atr[pka_idx].atr$w_type = 0;
        pka_atr[pka_idx].atr$l_addr = 0; /* NULL when DECC VAX gets fixed. */
    }

    nam = CC_RMS_NAM;               /* Initialize NAM[L]. */
    outfab->FAB_NAM = &nam;         /* Point FAB to NAM[L]. */

    /* Point the FAB-NAM[L] to the VMS-format directory file name. */

#ifdef NAML$C_MAXRSS

    outfab->fab$l_dna = (char *) -1;    /* Using NAML for default name. */
    outfab->fab$l_fna = (char *) -1;    /* Using NAML for file name. */

    /* Special ODS5-QIO-compatible name storage. */
    nam.naml$l_filesys_name = sys_nam;
    nam.naml$l_filesys_name_alloc = sizeof(sys_nam);

#endif /* NAML$C_MAXRSS */

    FAB_OR_NAML(*outfab, nam).FAB_OR_NAML_FNA = dir_name;
    FAB_OR_NAML(*outfab, nam).FAB_OR_NAML_FNS = strlen(dir_name);

    /* Expanded and resultant name storage. */
    nam.NAM_ESA = exp_nam;
    nam.NAM_ESS = sizeof(exp_nam);
    nam.NAM_RSA = res_nam;
    nam.NAM_RSS = sizeof(res_nam);

    status = sys$parse(outfab);
    if ( ERR(status) )
    {
        sprintf(warnmsg,
          "warning:  set-dir-attributes failed ($parse) for %s.\n",
          dir_name);
        vms_msg(__G__ warnmsg, status);
        retcode = PK_WARN;
        goto cleanup_exit;
    }

    /* Set the length in the device name descriptor. */
    pka_devdsc.dsc$w_length = (unsigned short) nam.NAM_DVI[0];

    /* Open a channel to the disk device. */
    status = sys$assign(&pka_devdsc, &pka_devchn, 0, 0);
    if ( ERR(status) )
    {
        sprintf(warnmsg,
          "warning:  set-dir-attributes failed ($assign) for %s.\n",
          dir_name);
        vms_msg(__G__ warnmsg, status);
        retcode = PK_WARN;
        goto cleanup_exit;
    }

    /* Move the directory ID from the NAM[L] to the FIB.
       Clear the FID in the FIB, as we're using the name.
    */
    for (i = 0; i < 3; i++)
    {
        pka_fib.FIB$W_DID[i] = nam.NAM_DID[i];
        pka_fib.FIB$W_FID[i] = 0;
    }

#ifdef NAML$C_MAXRSS

    /* Enable fancy name characters.  Note that "fancy" here does
       not include Unicode, for which there's no support elsewhere.
    */
    pka_fib.fib$v_names_8bit = 1;
    pka_fib.fib$b_name_format_in = FIB$C_ISL1;

    /* ODS5 Extended names used as input to QIO have peculiar
       encoding (perhaps to minimize storage?), so the special
       filesys_name result (typically containing fewer carets) must
       be used here.
    */
    pka_fnam.dsc$a_pointer = nam.naml$l_filesys_name;
    pka_fnam.dsc$w_length = nam.naml$l_filesys_name_size;

#else /* !NAML$C_MAXRSS */

    /* ODS2-only: Use the whole name. */
    pka_fnam.dsc$a_pointer = nam.NAM_L_NAME;
    pka_fnam.dsc$w_length = nam.NAM_B_NAME + nam.NAM_B_TYPE + nam.NAM_B_VER;

#endif /* ?NAML$C_MAXRSS */

    /* 2007-07-13 SMS.
     * Our freshly created directory can easily contain fewer files than
     * the original archived directory (for example, if not all the
     * files in the original directory were included in the archive), so
     * its size may differ from that of the archived directory.  Thus,
     * simply restoring the original RECATTR attributes structure, which
     * includes EFBLK (and so on) can cause "SYSTEM-W-BADIRECTORY, bad
     * directory file format" complaints.  Instead, we overwrite
     * selected archived attributes with current attributes, to avoid
     * setting obsolete/inappropriate attributes on the newly created
     * directory file.
     *
     * First, see if there is a RECATTR structure about which we need to
     * worry.
     */
    for (i = 0; pka_atr[i].atr$w_type != 0; i++)
    {
        if (pka_atr[i].atr$w_type == ATR$C_RECATTR)
        {
            /* We found a RECATTR structure which (we must assume) needs
             * adjustment.  Retrieve the RECATTR data for the existing
             * (newly created) directory file.
             */
            status = sys$qiow(0,                    /* event flag */
                              pka_devchn,           /* channel */
                              IO$_ACCESS,           /* function code */
                              &pka_acp_iosb,        /* IOSB */
                              0,                    /* AST address */
                              0,                    /* AST parameter */
                              &pka_fibdsc,          /* P1 = File Info Block */
                              &pka_fnam,            /* P2 = File name */
                              0,                    /* P3 = Rslt nm len */
                              0,                    /* P4 = Rslt nm str */
                              pka_recattr,          /* P5 = Attributes */
                              0);                   /* P6 (not used) */

            /* If initial success, then get the final status from the IOSB. */
            if ( !ERR(status) )
                status = pka_acp_iosb.status;

            if ( ERR(status) )
            {
                sprintf(warnmsg,
                  "warning:  set-dir-attributes failed ($qiow acc) for %s.\n",
                  dir_name);
                vms_msg(__G__ warnmsg, status);
                retcode = PK_WARN;
            }
            else
            {
                /* We should have valid RECATTR data.  Overwrite the
                 * critical bits of the archive RECATTR structure with
                 * the current bits.  The book says that an attempt to
                 * modify HIBLK will be ignored, and FFBYTE should
                 * always be zero, but safety is cheap.
                 */
                struct fatdef *ptr_recattr;

                ptr_recattr = (struct fatdef *) pka_atr[i].atr$l_addr;
                ptr_recattr->fat$l_hiblk =  pka_rattr.fat$l_hiblk;
                ptr_recattr->fat$l_efblk =  pka_rattr.fat$l_efblk;
                ptr_recattr->fat$w_ffbyte = pka_rattr.fat$w_ffbyte;
            }
        /* There should be only one RECATTR structure in the list, so
         * escape from the loop after the first/only one has been
         * processed.
         */
        break;
        }
    }

    /* Modify the file (directory) attributes. */
    status = sys$qiow(0,                            /* event flag */
                      pka_devchn,                   /* channel */
                      IO$_MODIFY,                   /* function code */
                      &pka_acp_iosb,                /* IOSB */
                      0,                            /* AST address */
                      0,                            /* AST parameter */
                      &pka_fibdsc,                  /* P1 = File Info Block */
                      &pka_fnam,                    /* P2 = File name */
                      0,                            /* P3 = Rslt nm len */
                      0,                            /* P4 = Rslt nm str */
                      pka_atr,                      /* P5 = Attributes */
                      0);                           /* P6 (not used) */

    /* If initial success, then get the final status from the IOSB. */
    if ( !ERR(status) )
        status = pka_acp_iosb.status;

    if ( ERR(status) )
    {
        sprintf(warnmsg,
          "warning:  set-dir-attributes failed ($qiow mod) for %s.\n",
          dir_name);
        vms_msg(__G__ warnmsg, status);
        retcode = PK_WARN;
    }
    sys$dassgn(pka_devchn);
cleanup_exit:
    free_up();                          /* Free FAB, XAB storage. */
    free(d);                            /* Free directory attribute storage. */
    G.extra_field = sav_ef_ptr;         /* Restore original pointer. */
    return retcode;
} /* end function set_direc_attribs() */

#endif /* SET_DIR_ATTRIB */



#ifdef TIMESTAMP

/* Nonzero if `y' is a leap year, else zero. */
#define leap(y) (((y) % 4 == 0 && (y) % 100 != 0) || (y) % 400 == 0)

/* Number of leap years from 1970 to `y' (not including `y' itself). */
#define nleap(y) (((y) - 1969) / 4 - ((y) - 1901) / 100 + ((y) - 1601) / 400)

/* Accumulated number of days from 01-Jan up to start of current month. */
static ZCONST short ydays[] = {
    0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334, 365
};

/***********************/
/* Function mkgmtime() */
/***********************/

static time_t mkgmtime(tm)
    struct tm *tm;
{
    time_t m_time;
    int yr, mo, dy, hh, mm, ss;
    unsigned days;

    yr = tm->tm_year - 70;
    mo = tm->tm_mon;
    dy = tm->tm_mday - 1;
    hh = tm->tm_hour;
    mm = tm->tm_min;
    ss = tm->tm_sec;

    /* calculate days from BASE to this year and add expired days this year */
    dy = (unsigned)dy + ((unsigned)yr * 365) + (unsigned)nleap(yr+1970) +
         (unsigned)ydays[mo] + ((mo > 1) && leap(yr+1970));

    /* convert date & time to seconds relative to 00:00:00, 01/01/1970 */
    return (time_t)((unsigned long)(unsigned)dy * 86400L +
                    (unsigned long)hh * 3600L +
                    (unsigned long)(mm * 60 + ss));

} /* end function mkgmtime() */



/*******************************/
/* Function dos_to_unix_time() */  /* only used for timestamping of archives */
/*******************************/

time_t dos_to_unix_time(dosdatetime)
    ulg dosdatetime;
{
    struct tm *ltm;             /* Local time. */
    time_t loctime;             /* The time_t value of local time. */
    time_t then;                /* The time to return. */
    long tzoffset_adj;          /* timezone-adjustment `remainder' */
    int bailout_cnt;            /* counter of tries for tz correction */

    then = time(NULL);
    ltm = localtime(&then);

    /* dissect date */
    ltm->tm_year = ((int)(dosdatetime >> 25) & 0x7f) + 80;
    ltm->tm_mon  = ((int)(dosdatetime >> 21) & 0x0f) - 1;
    ltm->tm_mday = ((int)(dosdatetime >> 16) & 0x1f);

    /* dissect time */
    ltm->tm_hour = (int)(dosdatetime >> 11) & 0x1f;
    ltm->tm_min  = (int)(dosdatetime >> 5) & 0x3f;
    ltm->tm_sec  = (int)(dosdatetime << 1) & 0x3e;

    loctime = mkgmtime(ltm);

    /* Correct for the timezone and any daylight savings time.
       The correction is verified and repeated when not correct, to
       take into account the rare case that a change to or from daylight
       savings time occurs between when it is the time in `tm' locally
       and when it is that time in Greenwich. After the second correction,
       the "timezone & daylight" offset should be correct in all cases. To
       be sure, we allow a third try, but then the loop is stopped. */
    bailout_cnt = 3;
    then = loctime;
    do {
      ltm = localtime(&then);
      tzoffset_adj = (ltm != NULL) ? (loctime - mkgmtime(ltm)) : 0L;
      if (tzoffset_adj == 0L)
        break;
      then += tzoffset_adj;
    } while (--bailout_cnt > 0);

    if ( (dosdatetime >= DOSTIME_2038_01_18) &&
         (then < (time_t)0x70000000L) )
        then = U_TIME_T_MAX;    /* saturate in case of (unsigned) overflow */
    if (then < (time_t)0L)      /* a converted DOS time cannot be negative */
        then = S_TIME_T_MAX;    /*  -> saturate at max signed time_t value */
    return then;

} /* end function dos_to_unix_time() */



/*******************************/
/*  Function uxtime2vmstime()  */
/*******************************/

static void uxtime2vmstime(  /* convert time_t value into 64 bit VMS bintime */
    time_t utimeval,
    long int binval[2] )
{
    time_t m_time = utimeval;
    struct tm *t = localtime(&m_time);

    if (t == (struct tm *)NULL) {
        /* time conversion error; use current time instead, hoping
           that localtime() does not reject it as well! */
        m_time = time(NULL);
        t = localtime(&m_time);
    }
    sprintf(timbuf, "%02d-%3s-%04d %02d:%02d:%02d.00",
            t->tm_mday, month[t->tm_mon], t->tm_year + 1900,
            t->tm_hour, t->tm_min, t->tm_sec);
    sys$bintim(&date_str, binval);
} /* end function uxtime2vmstime() */



/***************************/
/*  Function stamp_file()  */  /* adapted from VMSmunch...it just won't die! */
/***************************/

int stamp_file(fname, modtime)
    ZCONST char *fname;
    time_t modtime;
{
    int status;
    int i;
    static long int Cdate[2], Rdate[2], Edate[2], Bdate[2];
    static short int revisions;
#if defined(__DECC) || defined(__DECCXX)
#pragma __member_alignment __save
#pragma __nomember_alignment
#endif /* __DECC || __DECCXX */
    static union {
      unsigned short int value;
      struct {
        unsigned system : 4;
        unsigned owner : 4;
        unsigned group : 4;
        unsigned world : 4;
      } bits;
    } prot;
#if defined(__DECC) || defined(__DECCXX)
#pragma __member_alignment __restore
#endif /* __DECC || __DECCXX */
    static unsigned long uic;
    static struct fjndef jnl;

    static struct atrdef Atr[] = {
        {sizeof(pka_rattr), ATR$C_RECATTR, GVTC &pka_rattr},
        {sizeof(pka_uchar), ATR$C_UCHAR, GVTC &pka_uchar},
        {sizeof(Cdate), ATR$C_CREDATE, GVTC &Cdate[0]},
        {sizeof(Rdate), ATR$C_REVDATE, GVTC &Rdate[0]},
        {sizeof(Edate), ATR$C_EXPDATE, GVTC &Edate[0]},
        {sizeof(Bdate), ATR$C_BAKDATE, GVTC &Bdate[0]},
        {sizeof(revisions), ATR$C_ASCDATES, GVTC &revisions},
        {sizeof(prot), ATR$C_FPRO, GVTC &prot},
        {sizeof(uic), ATR$C_UIC, GVTC &uic},
        {sizeof(jnl), ATR$C_JOURNAL, GVTC &jnl},
        {0, 0, 0}
    };

    fileblk = cc$rms_fab;               /* Initialize FAB. */
    nam = CC_RMS_NAM;                   /* Initialize NAM[L]. */
    fileblk.FAB_NAM = &nam;             /* Point FAB to NAM[L]. */

#ifdef NAML$C_MAXRSS

    fileblk.fab$l_dna = (char *) -1;    /* Using NAML for default name. */
    fileblk.fab$l_fna = (char *) -1;    /* Using NAML for file name. */

    /* Special ODS5-QIO-compatible name storage. */
    nam.naml$l_filesys_name = sys_nam;
    nam.naml$l_filesys_name_alloc = sizeof(sys_nam);

#endif /* NAML$C_MAXRSS */

    FAB_OR_NAML(fileblk, nam).FAB_OR_NAML_FNA = (char *)fname;
    FAB_OR_NAML(fileblk, nam).FAB_OR_NAML_FNS = strlen(fname);

    nam.NAM_ESA = exp_nam;
    nam.NAM_ESS = sizeof(exp_nam);
    nam.NAM_RSA = res_nam;
    nam.NAM_RSS = sizeof(res_nam);

    if ( ERR(status = sys$parse(&fileblk)) )
    {
        vms_msg(__G__ "stamp_file: sys$parse failed.\n", status);
        return -1;
    }

    pka_devdsc.dsc$w_length = (unsigned short)nam.NAM_DVI[0];

    if ( ERR(status = sys$assign(&pka_devdsc, &pka_devchn, 0, 0)) )
    {
        vms_msg(__G__ "stamp_file: sys$assign failed.\n", status);
        return -1;
    }

    /* Load the descriptor with the appropriate name data: */
#ifdef NAML$C_MAXRSS

    /* Enable fancy name characters.  Note that "fancy" here does
       not include Unicode, for which there's no support elsewhere.
    */
    pka_fib.fib$v_names_8bit = 1;
    pka_fib.fib$b_name_format_in = FIB$C_ISL1;

    /* ODS5 Extended names used as input to QIO have peculiar
       encoding (perhaps to minimize storage?), so the special
       filesys_name result (typically containing fewer carets) must
       be used here.
    */
    pka_fnam.dsc$a_pointer = nam.naml$l_filesys_name;
    pka_fnam.dsc$w_length = nam.naml$l_filesys_name_size;

#else /* !NAML$C_MAXRSS */

    /* Extract only the name.type;version. */
    pka_fnam.dsc$a_pointer = nam.NAM_L_NAME;
    pka_fnam.dsc$w_length = nam.NAM_B_NAME + nam.NAM_B_TYPE + nam.NAM_B_VER;

#endif /* ?NAML$C_MAXRSS */

    /* Move the directory ID from the NAM[L] to the FIB.
       Clear the FID in the FIB, as we're using the name.
    */
    for (i = 0; i < 3; i++)
    {
        pka_fib.FIB$W_DID[i] = nam.NAM_DID[i];
        pka_fib.FIB$W_FID[i] = 0;
    }

    /* Use the IO$_ACCESS function to return info about the file.
       This way, the file is not opened, and the expiration and
       revision dates are not modified.
    */
    status = sys$qiow(0, pka_devchn, IO$_ACCESS,
                      &pka_acp_iosb, 0, 0,
                      &pka_fibdsc, &pka_fnam, 0, 0, Atr, 0);

    if ( !ERR(status) )
        status = pka_acp_iosb.status;

    if ( ERR(status) )
    {
        vms_msg(__G__ "[ Access file QIO failed. ]\n", status);
        sys$dassgn(pka_devchn);
        return -1;
    }

    uxtime2vmstime(modtime, Cdate);
    memcpy(Rdate, Cdate, sizeof(Cdate));

    /* Note: Part of the FIB was cleared by earlier QIOW, so reset it. */
    pka_fib.FIB$L_ACCTL = FIB$M_NORECORD;

    /* Move the directory ID from the NAM[L] to the FIB.
       Clear the FID in the FIB, as we're using the name.
    */
    for (i = 0; i < 3; i++)
    {
        pka_fib.FIB$W_DID[i] = nam.NAM_DID[i];
        pka_fib.FIB$W_FID[i] = 0;
    }

    /* Use the IO$_MODIFY function to change info about the file */
    /* Note, used this way, the file is not opened, however this would */
    /* normally cause the expiration and revision dates to be modified. */
    /* Using FIB$M_NORECORD prohibits this from happening. */
    status = sys$qiow(0, pka_devchn, IO$_MODIFY,
                      &pka_acp_iosb, 0, 0,
                      &pka_fibdsc, &pka_fnam, 0, 0, Atr, 0);

    if ( !ERR(status) )
        status = pka_acp_iosb.status;

    if ( ERR(status) )
    {
        vms_msg(__G__ "[ Modify file QIO failed. ]\n", status);
        sys$dassgn(pka_devchn);
        return -1;
    }

    if ( ERR(status = sys$dassgn(pka_devchn)) )
    {
        vms_msg(__G__ "stamp_file: sys$dassgn failed.\n", status);
        return -1;
    }

    return 0;

} /* end function stamp_file() */

#endif /* TIMESTAMP */



#ifdef DEBUG
#if 0   /* currently not used anywhere ! */
void dump_rms_block(p)
    unsigned char *p;
{
    unsigned char bid, len;
    int err;
    char *type;
    char buf[132];
    int i;

    err = 0;
    bid = p[0];
    len = p[1];
    switch (bid)
    {
        case FAB$C_BID:
            type = "FAB";
            break;
        case XAB$C_ALL:
            type = "xabALL";
            break;
        case XAB$C_KEY:
            type = "xabKEY";
            break;
        case XAB$C_DAT:
            type = "xabDAT";
            break;
        case XAB$C_RDT:
            type = "xabRDT";
            break;
        case XAB$C_FHC:
            type = "xabFHC";
            break;
        case XAB$C_PRO:
            type = "xabPRO";
            break;
        default:
            type = "Unknown";
            err = 1;
            break;
    }
    printf("Block @%08X of type %s (%d).", p, type, bid);
    if (err)
    {
        printf("\n");
        return;
    }
    printf(" Size = %d\n", len);
    printf(" Offset - Hex - Dec\n");
    for (i = 0; i < len; i += 8)
    {
        int j;

        printf("%3d - ", i);
        for (j = 0; j < 8; j++)
            if (i + j < len)
                printf("%02X ", p[i + j]);
            else
                printf("   ");
        printf(" - ");
        for (j = 0; j < 8; j++)
            if (i + j < len)
                printf("%03d ", p[i + j]);
            else
                printf("    ");
        printf("\n");
    }
}

#endif                          /* never */
#endif                          /* DEBUG */



static char vms_msgbuf[256];            /* VMS-specific error message. */
static $DESCRIPTOR(vms_msgbuf_dscr, vms_msgbuf);


char *vms_msg_text(void)
{
    return vms_msgbuf;
}


static int vms_msg_fetch(int status)
{
    int msglen = 0;
    int sts;

    sts = lib$sys_getmsg(&status, &msglen, &vms_msgbuf_dscr, 0, 0);

    vms_msgbuf[msglen] = '\0';
    return sts;
}


static void vms_msg(__GPRO__ ZCONST char *string, int status)
{
    if (ERR(vms_msg_fetch(status)))
        Info(slide, 1, ((char *)slide,
             "%s[ VMS status = %d ]\n", string, status));
    else
        Info(slide, 1, ((char *)slide,
             "%s[ %s ]\n", string, vms_msgbuf));
}



#ifndef SFX

/* 2004-11-23 SMS.
 * Changed to return the resulting file name even when sys$search()
 * fails.  Before, if the user specified "fred.zip;4" and there was
 * none, the error message would complain:
 *    cannot find either fred.zip;4 or fred.zip;4.zip.
 * when it wasn't really looking for "fred.zip;4.zip".
 */
/* 2005-08-11 SPC.
 * The calling interface for the VMS version of do_wild() differs from all
 * other implementations in the way it returns status info.
 * There are three return states:
 * a) pointer to buffer with non-zero-length string
 *    - canonical full filespec of existing file (search succeeded).
 * b) pointer to buffer with zero-length string
 *    - initial file search has failed, extended VMS error info is available
 *      through call to vms_msg_text().
 * c) NULL pointer
 *    - repeated file search has failed, because
 *      i)   the list of matches for the pattern has been exhausted after at
 *           least one successful attempt.
 *      ii)  a second attempt for a failed initial pattern (where do_wild()
 *           has returned a zero-length string) was tried and failed again.
 */
char *do_wild( __G__ wld )
    __GDEF
    ZCONST char *wld;
{
    int status;

    static char filenam[NAM_MAXRSS + 1];
    static char efn[NAM_MAXRSS];
    static char last_wild[NAM_MAXRSS + 1];
    static struct FAB fab;
    static struct NAM_STRUCT nam;
    static int first_call = 1;
    static ZCONST char deflt[] = "[]*.ZIP";

    if ( first_call || strcmp(wld, last_wild) )
    {   /* (Re)Initialize everything */

        strcpy( last_wild, wld );

        fab = cc$rms_fab;               /* Initialize FAB. */
        nam = CC_RMS_NAM;               /* Initialize NAM[L]. */
        fab.FAB_NAM = &nam;             /* Point FAB to NAM[L]. */

#ifdef NAML$C_MAXRSS

        fab.fab$l_dna = (char *) -1;    /* Using NAML for default name. */
        fab.fab$l_fna = (char *) -1;    /* Using NAML for file name. */

#endif /* NAML$C_MAXRSS */

        FAB_OR_NAML(fab, nam).FAB_OR_NAML_DNA = (char *) deflt;
        FAB_OR_NAML(fab, nam).FAB_OR_NAML_DNS = sizeof(deflt) - 1;

        FAB_OR_NAML(fab, nam).FAB_OR_NAML_FNA = last_wild;
        FAB_OR_NAML(fab, nam).FAB_OR_NAML_FNS = strlen(last_wild);

        nam.NAM_ESA = efn;
        nam.NAM_ESS = sizeof(efn)-1;
        nam.NAM_RSA = filenam;
        nam.NAM_RSS = sizeof(filenam)-1;

        first_call = 0;

        /* 2005-08-08 SMS.
         * Parse the file spec.  If sys$parse() fails, save the VMS
         * error message for later use, and return an empty string.
         */
        nam.NAM_NOP = NAM_M_SYNCHK;     /* Syntax-only analysis. */
        if ( !OK(status = sys$parse(&fab)) )
        {
            vms_msg_fetch(status);
            filenam[0] = '\0';          /* Initialization failed */
            return filenam;
        }

        /* 2005-11-16 SMS.
         * If syntax-only parse worked, re-parse normally so that
         * sys$search() will work properly.  Regardless of parse error,
         * leave filenam[] as-was.
         */
        nam.NAM_NOP = 0;                /* Normal analysis. */
        if ( OK(status = sys$parse(&fab)) )
        {
            status = sys$search(&fab);
        }

        if ( !OK(status) )
        {
            /* Save the VMS error message for later use. */
            vms_msg_fetch(status);
        }
    }
    else
    {
        if ( !OK(sys$search(&fab)) )
        {
            first_call = 1;             /* Reinitialize next time */
            return (char *)NULL;
        }
    }
    filenam[nam.NAM_RSL] = '\0';        /* Add the NUL terminator. */
    return filenam;

} /* end function do_wild() */

#endif /* !SFX */



static ulg unix_to_vms[8]={ /* Map from UNIX rwx to VMS rwed */
                            /* Note that unix w bit is mapped to VMS wd bits */
                                                              /* no access */
    XAB$M_NOREAD | XAB$M_NOWRITE | XAB$M_NODEL | XAB$M_NOEXE,    /* --- */
    XAB$M_NOREAD | XAB$M_NOWRITE | XAB$M_NODEL,                  /* --x */
    XAB$M_NOREAD |                               XAB$M_NOEXE,    /* -w- */
    XAB$M_NOREAD,                                                /* -wx */
                   XAB$M_NOWRITE | XAB$M_NODEL | XAB$M_NOEXE,    /* r-- */
                   XAB$M_NOWRITE | XAB$M_NODEL,                  /* r-x */
                                                 XAB$M_NOEXE,    /* rw- */
    0                                                            /* rwx */
                                                              /* full access */
};

#define SETDFPROT   /* We are using undocumented VMS System Service     */
                    /* SYS$SETDFPROT here. If your version of VMS does  */
                    /* not have that service, undef SETDFPROT.          */
                    /* IM: Maybe it's better to put this to Makefile    */
                    /* and DESCRIP.MMS */
#ifdef SETDFPROT
extern int sys$setdfprot();
#endif

int mapattr(__G)
    __GDEF
{
    ulg tmp = G.crec.external_file_attributes;
    ulg theprot;
    static ulg  defprot = (ulg)-1L,
                sysdef, owndef, grpdef, wlddef; /* Default protection fields */

    /* IM: The only field of XABPRO we need to set here is */
    /*     file protection, so we need not to change type */
    /*     of G.pInfo->file_attr. WORD is quite enough. */

    if ( defprot == (ulg)-1L )
    {
        /*
         * First time here -- Get user default settings
         */

#ifdef SETDFPROT    /* Undef this if linker cat't resolve SYS$SETDFPROT */
        defprot = (ulg)0L;
        if ( !ERR(sys$setdfprot(0, &defprot)) )
        {
            sysdef = defprot & ( (1L<<XAB$S_SYS)-1 ) << XAB$V_SYS;
            owndef = defprot & ( (1L<<XAB$S_OWN)-1 ) << XAB$V_OWN;
            grpdef = defprot & ( (1L<<XAB$S_GRP)-1 ) << XAB$V_GRP;
            wlddef = defprot & ( (1L<<XAB$S_WLD)-1 ) << XAB$V_WLD;
        }
        else
#endif /* SETDFPROT */
        {
            umask(defprot = umask(0));
            defprot = ~defprot;
            wlddef = unix_to_vms[defprot & 07] << XAB$V_WLD;
            grpdef = unix_to_vms[(defprot>>3) & 07] << XAB$V_GRP;
            owndef = unix_to_vms[(defprot>>6) & 07] << XAB$V_OWN;
            sysdef = owndef >> (XAB$V_OWN - XAB$V_SYS);
            defprot = sysdef | owndef | grpdef | wlddef;
        }
    }

    switch (G.pInfo->hostnum) {
        case AMIGA_:
            tmp = (unsigned)(tmp>>16 & 0x0f);   /* Amiga RWED bits */
            G.pInfo->file_attr =  (tmp << XAB$V_OWN) |
                                   grpdef | sysdef | wlddef;
            break;

        case THEOS_:
            tmp &= 0xF1FFFFFFL;
            if ((tmp & 0xF0000000L) != 0x40000000L)
                tmp &= 0x01FFFFFFL;     /* not a dir, mask all ftype bits */
            else
                tmp &= 0x41FFFFFFL;     /* leave directory bit as set */
            /* fall through! */

        case UNIX_:
        case VMS_:  /*IM: ??? Does VMS Zip store protection in UNIX format ?*/
                    /* GRR:  Yup.  Bad decision on my part... */
        case ACORN_:
        case ATARI_:
        case ATHEOS_:
        case BEOS_:
        case QDOS_:
        case TANDEM_:
            {
              int r = FALSE;
              unsigned uxattr = (unsigned)(tmp >> 16);  /* drwxrwxrwx */

              if (uxattr == 0 && G.extra_field) {
                /* Some (non-Info-ZIP) implementations of Zip for Unix and
                 * VMS (and probably others ??) leave 0 in the upper 16-bit
                 * part of the external_file_attributes field. Instead, they
                 * store file permission attributes in some e.f. block.
                 * As a work-around, we search for the presence of one of
                 * these extra fields and fall back to the MSDOS compatible
                 * part of external_file_attributes if one of the known
                 * e.f. types has been detected.
                 * Later, we might implement extraction of the permission
                 * bits from the VMS extra field. But for now, the work-around
                 * should be sufficient to provide "readable" extracted files.
                 * (For ASI Unix e.f., an experimental remap of the e.f.
                 * mode value IS already provided!)
                 */
                ush ebID;
                unsigned ebLen;
                uch *ef = G.extra_field;
                unsigned ef_len = G.crec.extra_field_length;

                while (!r && ef_len >= EB_HEADSIZE) {
                    ebID = makeword(ef);
                    ebLen = (unsigned)makeword(ef+EB_LEN);
                    if (ebLen > (ef_len - EB_HEADSIZE))
                        /* discoverd some e.f. inconsistency! */
                        break;
                    switch (ebID) {
                      case EF_ASIUNIX:
                        if (ebLen >= (EB_ASI_MODE+2)) {
                            uxattr =
                              (unsigned)makeword(ef+(EB_HEADSIZE+EB_ASI_MODE));
                            /* force stop of loop: */
                            ef_len = (ebLen + EB_HEADSIZE);
                            break;
                        }
                        /* else: fall through! */
                      case EF_PKVMS:
                        /* "found nondecypherable e.f. with perm. attr" */
                        r = TRUE;
                      default:
                        break;
                    }
                    ef_len -= (ebLen + EB_HEADSIZE);
                    ef += (ebLen + EB_HEADSIZE);
                }
              }
              if (!r) {
#ifdef SYMLINKS
                  /* Check if the file is a (POSIX-compatible) symbolic link.
                   * We restrict symlink support to those "made-by" hosts that
                   * are known to support symbolic links.
                   */
                  G.pInfo->symlink = S_ISLNK(uxattr) &&
                                     SYMLINK_HOST(G.pInfo->hostnum);
#endif
                  theprot  = (unix_to_vms[uxattr & 07] << XAB$V_WLD)
                           | (unix_to_vms[(uxattr>>3) & 07] << XAB$V_GRP)
                           | (unix_to_vms[(uxattr>>6) & 07] << XAB$V_OWN);
                  if ( uxattr & 0x4000 )
                      /* Directory -- set D bits */
                      theprot |= (XAB$M_NODEL << XAB$V_SYS)
                              | (XAB$M_NODEL << XAB$V_OWN)
                              | (XAB$M_NODEL << XAB$V_GRP)
                              | (XAB$M_NODEL << XAB$V_WLD);
                  G.pInfo->file_attr = theprot;
                  break;
              }
            }
            /* fall through! */

        /* all remaining cases:  expand MSDOS read-only bit into write perms */
        case FS_FAT_:
        case FS_HPFS_:
        case FS_NTFS_:
        case MAC_:
        case TOPS20_:
        default:
            theprot = defprot;
            if ( tmp & 1 )   /* Test read-only bit */
            {   /* Bit is set -- set bits in all fields */
                tmp = XAB$M_NOWRITE | XAB$M_NODEL;
                theprot |= (tmp << XAB$V_SYS) | (tmp << XAB$V_OWN) |
                           (tmp << XAB$V_GRP) | (tmp << XAB$V_WLD);
            }
            G.pInfo->file_attr = theprot;
            break;
    } /* end switch (host-OS-created-by) */

    return 0;

} /* end function mapattr() */


#define PATH_DEFAULT "SYS$DISK:[]"

/* dest_struct_level()

      Returns file system structure level for argument, negative on
      error.
*/

int dest_struct_level(char *path)
{
    int acp_code;

#ifdef DVI$C_ACP_F11V5

    /* Should know about ODS5 file system.  Do actual check.
       (This should be non-VAX with __CRTL_VER >= 70200000.)
    */

    int sts;

    struct FAB fab;
    struct NAM_STRUCT nam;
    char e_name[NAM_MAXRSS + 1];

    struct dsc$descriptor_s dev_descr =
     { 0, DSC$K_DTYPE_T, DSC$K_CLASS_S, 0 };

    fab = cc$rms_fab;                   /* Initialize FAB. */
    nam = CC_RMS_NAM;                   /* Initialize NAM[L]. */
    fab.FAB_NAM = &nam;                 /* Point FAB to NAM[L]. */

#ifdef NAML$C_MAXRSS

    fab.fab$l_dna = (char *) -1;        /* Using NAML for default name. */
    fab.fab$l_fna = (char *) -1;        /* Using NAML for file name. */

#endif /* NAML$C_MAXRSS */

    FAB_OR_NAML(fab, nam).FAB_OR_NAML_DNA = PATH_DEFAULT;
    FAB_OR_NAML(fab, nam).FAB_OR_NAML_DNS = strlen(PATH_DEFAULT);

    FAB_OR_NAML(fab, nam).FAB_OR_NAML_FNA = path;
    FAB_OR_NAML(fab, nam).FAB_OR_NAML_FNS = strlen(path);

    nam.NAM_ESA = e_name;
    nam.NAM_ESS = sizeof(e_name) - 1;

    nam.NAM_NOP = NAM_M_SYNCHK;         /* Syntax-only analysis. */
    sts = sys$parse(&fab);

    if ((sts & STS$M_SUCCESS) == STS$K_SUCCESS)
    {
        /* Load resultant device name into device descriptor. */
        dev_descr.dsc$a_pointer = nam.NAM_L_DEV;
        dev_descr.dsc$w_length = nam.NAM_B_DEV;

        /* Get filesystem type code.
           (Text results for this item code have been unreliable.)
        */
        sts = lib$getdvi(&((int) DVI$_ACPTYPE),
                         0,
                         &dev_descr,
                         &acp_code,
                         0,
                         0);

        if ((sts & STS$M_SUCCESS) != STS$K_SUCCESS)
        {
            acp_code = -2;
        }
    }
    else
    {
        acp_code = -1;
    }

#else /* !DVI$C_ACP_F11V5 */

/* Too old for ODS5 file system.  Return level 2. */

    acp_code = DVI$C_ACP_F11V2;

#endif /* ?DVI$C_ACP_F11V5 */

    return acp_code;
}

/* 2005-02-12 SMS.
   Note that these name conversion functions do no length checking.
   Buffer overflows are possible.
*/

static void adj_dir_name_ods2(char *dest, char *src, int src_len)
{
    /* The source string (src) typically extends beyond the directory
       segment of interest, hence the confining src_len argument.
    */
    unsigned char uchr;
    unsigned char prop;
    char * src_last;

    for (src_last = src + src_len; src < src_last; src++)
    {
        prop = char_prop[uchr = *src];  /* Get source char, properties. */
        if ((prop & 2) != 0)            /* Up-case lower case. */
        {
            uchr -= ('a' - 'A');        /* (Simple-fast is adequate.) */
        }
        else if ((prop & 1) == 0)       /* Replace invalid char */
        {
            uchr = '_';                 /* with "_". */
        }
        *dest++ = uchr;                 /* Store good char. */
    }
    *dest = '\0';                       /* Terminate destination. */
}


static void adj_dir_name_ods5(char *dest, char *src, int src_len)
{
    /* The source string (src) typically extends beyond the directory
       segment of interest, hence the confining src_len argument.
    */
    unsigned char uchr;
    unsigned char prop;
    char * src_last;

    for (src_last = src + src_len; src < src_last; src++)
    {
        prop = char_prop[uchr = *src];          /* Get source char, props. */
        prop = char_prop[uchr];                 /* Get source char props. */
        if ((prop & (32+8+4)) != 0)             /* Escape 1-char, including */
        {                                       /* SP and dot. */
            *dest++ = '^';                      /* Insert caret. */
            if ((prop & 8) != 0)                /* Replace SP with "_". */
            {
                uchr = '_';
            }
            else if (uchr == '?')
            {
                uchr = '/';                     /* Replace "?" with "/". */
            }
        }
        else if ((prop & 64) != 0)              /* Escape hex-hex. */
        {
            *dest++ = '^';                      /* Insert caret. */
            *dest++ = hex_digit[uchr >> 4];     /* First hex digit. */
            uchr = hex_digit[uchr & 15];        /* Second hex digit. */
        }
        else if ((prop & 16) == 0)              /* Replace invalid with "_". */
        {
            uchr = '_';
        }
        *dest++ = uchr;                         /* Put good (or last) char. */
    }
    *dest = '\0';                               /* Terminate destination. */
}


static void adj_file_name_ods2(char *dest, char *src)
{
    unsigned char uchr;
    unsigned char prop;
    char *endp;
    char *versionp;
    char *last_dot;

    endp = src + strlen(src);   /* Pointer to the NUL-terminator of src. */
    /* Starting at the end, find the last non-decimal-digit. */
    versionp = endp;
    while ((--versionp >= src) && isdigit(*versionp));

    /* Left-most non-digit of a valid version is ";" (or perhaps "."). */
    if ((*versionp != ';') && ((uO.Y_flag == 0) || (*versionp != '.')))
    {
        /* No valid version.  The last dot is the last dot. */
        versionp = endp;
    }
    else
    {   /* Some kind of valid version. */
        if (!uO.V_flag)                 /* Not -V, so cut off version. */
        {
            *versionp = '\0';
        }
        else if (*versionp == '.')
        {
            *versionp = ';';            /* Replace version dot with ";". */
        }
    }

    /* 2008-11-04 SMS.
     * Simplified the scheme here to escape all non-last dots.  This
     * should work when Zip works correctly (Zip 3.1).
     * Note that if no last dot is found, the non-last-dot test below
     * will always fail, but that's not a problem.
     */

    /* Find the last dot (if any). */
    last_dot = versionp;
    while ((--last_dot >= src) && (*last_dot != '.'));

    /* Critical features having been located, transform the name. */
    while ((uchr = *src++) != '\0')     /* Get source character. */
    {
        /* Note that "src" has been incremented, affecting "src <=". */
        prop = char_prop[uchr];         /* Get source char properties. */
        if ((prop & 2) != 0)            /* Up-case lower case. */
        {
            uchr -= ('a' - 'A');        /* (Simple-fast is adequate.) */
        }
        else if ((prop & 4) != 0)       /* Dot. */
        {
            if (src <= last_dot)        /* Replace non-last dot */
            {
                uchr = '_';             /* with "_". */
            }
        }
        else if ((prop & 1) == 0)       /* Replace SP or invalid char, */
        {
            if (src <= versionp)        /* if not in version, */
            {
                uchr = '_';             /* with "_". */
            }
        }
        *dest++ = uchr;                 /* Store good char. */
    }
    *dest = '\0';                       /* Terminate destination. */
}


static void adj_file_name_ods5(char *dest, char *src)
{
    unsigned char uchr;
    unsigned char prop;
    char *endp;
    char *versionp;
    char *last_dot;

    endp = src + strlen(src);   /* Pointer to the NUL-terminator of src. */
    /* Starting at the end, find the last non-decimal-digit. */
    versionp = endp;
    while ((--versionp >= src) && isdigit(*versionp));

    /* Left-most non-digit of a valid version is ";" (or perhaps "."). */
    if ((*versionp != ';') && ((uO.Y_flag == 0) || (*versionp != '.')))
    {
        /* No valid version.  The last dot is the last dot. */
        versionp = endp;
    }
    else
    {   /* Some kind of valid version. */
        if (!uO.V_flag)                 /* Not -V, so cut off version. */
        {
            *versionp = '\0';
        }
        else if (*versionp == '.')
        {
            *versionp = ';';            /* Replace version dot with ";". */
        }
    }

    /* 2008-11-04 SMS.
     * Simplified the scheme here to escape all non-last dots.  This
     * should work when Zip works correctly (Zip 3.1).
     * Note that if no last dot is found, the non-last-dot test below
     * will always fail, but that's not a problem.
     */

    /* Find the last dot (if any). */
    last_dot = versionp;
    while ((--last_dot >= src) && (*last_dot != '.'));

    /* Critical features having been located, transform the name. */
    while ((uchr = *src++) != '\0')             /* Get source character. */
    {
        /* Note that "src" has been incremented, affecting "src <=". */
        prop = char_prop[uchr];                 /* Get source char props. */
        if ((prop & (32+8)) != 0)               /* Escape 1-char, including */
        {                                       /* SP (but not dot). */
            if (src <= versionp)                /* No escapes for version. */
            {
                *dest++ = '^';                  /* Insert caret. */
                if ((prop & 8) != 0)            /* Replace SP with "_". */
                {
                    uchr = '_';
                }
                else if (uchr == '?')
                {
                    uchr = '/';                 /* Replace "?" with "/". */
                }
            }
        }
        else if ((prop & 4) != 0)               /* Dot. */
        {
            if (src <= last_dot)                /* Escape non-last dot */
            {
                *dest++ = '^';                  /* Insert caret. */
            }
        }
        else if ((prop & 64) != 0)              /* Escape hex-hex. */
        {
            *dest++ = '^';                      /* Insert caret. */
            *dest++ = hex_digit[uchr >> 4];     /* First hex digit. */
            uchr = hex_digit[uchr & 15];        /* Second hex digit. */
        }
        else if ((prop & 16) == 0)              /* Replace invalid with "_". */
        {
            uchr = '_';
        }
        *dest++ = uchr;                         /* Put good (or last) char. */
    }
    *dest = '\0';                               /* Terminate destination. */
}



#   define FN_MASK   7
#   define USE_DEFAULT  (FN_MASK+1)

/*
 * Checkdir function codes:
 *      ROOT        -   set root path from unzip qq d:[dir]
 *      INIT        -   get ready for "filename"
 *      APPEND_DIR  -   append pathcomp
 *      APPEND_NAME -   append filename
 *      APPEND_NAME | USE_DEFAULT   -    expand filename using collected path
 *      GETPATH     -   return resulting filespec
 *      END         -   free dynamically allocated space prior to program exit
 */

static int created_dir;
static int dest_file_sys_level;
static int ods2_names = -1;

int mapname(__G__ renamed)
        /* returns: */
        /* MPN_OK if no error, */
        /* MPN_INF_TRUNC if caution (filename trunc), */
        /* MPN_INF_SKIP if warning (skip file, dir doesn't exist), */
        /* MPN_ERR_SKIP if error (skip file), */
        /* MPN_CREATED_DIR if has created directory, */
        /* MPN_VOL_LABEL if path was volume label (skip it) */
        /* MPN_NOMEM if no memory (skip file) */
    __GDEF
    int renamed;
{
    char pathcomp[FILNAMSIZ];       /* Path-component buffer. */
    char *last_slash;               /* Last slash in path. */
    char *next_slash;               /* Next slash in path. */
    int  dir_len;                   /* Length of a directory segment. */

    char *cp = (char *)NULL;        /* character pointer */
    int killed_ddot = FALSE;        /* Set when skipping "../" pathcomp. */
    int error = MPN_OK;

    if ( renamed )
    {
        if ( !(error = checkdir(__G__ pathcomp, APPEND_NAME | USE_DEFAULT)) )
            strcpy(G.filename, pathcomp);
        return error;
    }

/*---------------------------------------------------------------------------
    Initialize various pointers and counters and stuff.
  ---------------------------------------------------------------------------*/

    if (G.pInfo->vollabel)
        return MPN_VOL_LABEL;   /* can't set disk volume labels on VMS */

    /* can create path as long as not just freshening, or if user told us */
    G.create_dirs = !uO.fflag;

    created_dir = FALSE;        /* not yet */

    /* If not yet known, determine the destination file system type
       (ODS2 or ODS5).  (If the user specified a destination, we should
       already have this, so use the default destination.)
    */
    if (ods2_names < 0)
    {
        /* If user doesn't force ODS2, set flag according to destination. */
        if (uO.ods2_flag == 0)
        {
            ods2_names =
             (dest_struct_level(PATH_DEFAULT) <= DVI$C_ACP_F11V2);
        }
        else
        {
            ods2_names = 1;     /* User demands ODS2 names. */
        }
    }

/* GRR:  for VMS, convert to internal format now or later? or never? */
    if (checkdir(__G__ pathcomp, INIT) == 10)
        return MPN_NOMEM;       /* Initialize path buffer, unless no memory. */

    /* Locate and treat directory segments one at a time.
       When pointer exceeds last_slash, then directory segments are
       done, and only the name (with version?) remains.
    */

    *pathcomp = '\0';           /* Initialize translation buffer. */
    last_slash = strrchr(G.filename, '/');      /* Find last slash. */

    if (uO.jflag)               /* If junking directories, */
        cp = last_slash;        /* start at (will be after) the last slash. */

    if (cp == NULL)             /* If no '/', or keeping directories, */
        cp = G.filename;        /* start at the front of the pathname. */
    else                        /* Else, with directories to junk, */
        ++cp;                   /* start after the last slash. */

    /* Loop through the directory segments. */
    while (cp < last_slash)
    {
        next_slash = strchr(cp, '/');  /* Find the next slash. */
        dir_len = next_slash- cp;

        /* Filter out unacceptable directories. */
        if ((dir_len == 2) && (strncmp(cp, "..", 2) == 0))
        {   /* Double dot. */
            if (!uO.ddotflag)           /* Not allowed.  Skip it. */
            {
                dir_len = 0;
                killed_ddot = TRUE;     /* Record skipping double-dot. */
            }
        }
        else if ((dir_len == 1) && (strncmp(cp, ".", 1) == 0))
        {   /* Single dot.  No-op.  Skip it. */
            dir_len = 0;
        }

        /* If non-null, acceptable directory, then process it. */
        if (dir_len > 0)
        {
            if (ods2_names)     /* Make directory name ODS2-compliant. */
            {
                adj_dir_name_ods2(pathcomp, cp, dir_len);
            }
            else                /* Make directory name ODS5-compliant. */
            {
                adj_dir_name_ods5(pathcomp, cp, dir_len);
            }
            if (((error = checkdir(__G__ pathcomp, APPEND_DIR))
                 & MPN_MASK) > MPN_INF_TRUNC)
                return error;
        }
        cp = next_slash+ 1;     /* Continue at the next name segment. */
    } /* end while loop */

    /* Show warning when stripping insecure "parent dir" path components */
    if (killed_ddot && QCOND2) {
        Info(slide, 0, ((char *)slide,
          "warning:  skipped \"../\" path component(s) in %s\n",
          FnFilter1(G.filename)));
        if (!(error & ~MPN_MASK))
            error = (error & MPN_MASK) | PK_WARN;
    }

    /* If there is one, adjust the name.type;version segment. */
    if (strlen(cp) == 0)
    {
        /* Directory only, no file name.  Create the directory, as needed.
           Report directory creation to user.
        */
        checkdir(__G__ "", APPEND_NAME);   /* create directory, if not found */
        checkdir(__G__ G.filename, GETPATH);
        if (created_dir) {
            if (QCOND2) {
                Info(slide, 0, ((char *)slide, "   creating: %s\n",
                  FnFilter1(G.filename)));
            }
            /* set dir time (note trailing '/') */
            return (error & ~MPN_MASK) | MPN_CREATED_DIR;
        }
        /* dir existed already; don't look for data to extract */
        return (error & ~MPN_MASK) | MPN_INF_SKIP;
    }

    /* Process the file name. */
    if (ods2_names)     /* Make file name ODS2-compliant. */
    {
        adj_file_name_ods2(pathcomp, cp);
    }
    else                /* Make file name ODS5-compliant. */
    {
        adj_file_name_ods5(pathcomp, cp);
    }

    checkdir(__G__ pathcomp, APPEND_NAME);  /* returns 1 if truncated: care? */
    checkdir(__G__ G.filename, GETPATH);

    return error;

} /* end function mapname() */



int checkdir(__G__ pathcomp, fcn)
/*
 * returns:
 *  MPN_OK          - no problem detected
 *  MPN_INF_TRUNC   - (on APPEND_NAME) truncated filename
 *  MPN_INF_SKIP    - path doesn't exist, not allowed to create
 *  MPN_ERR_SKIP    - path doesn't exist, tried to create and failed; or path
 *                    exists and is not a directory, but is supposed to be
 *  MPN_ERR_TOOLONG - path is too long
 *  MPN_NOMEM       - can't allocate memory for filename buffers
 */
    __GDEF
    char *pathcomp;
    int fcn;
{
    int function=fcn & FN_MASK;
    static char pathbuf[FILNAMSIZ];

    /* previously created directory (initialized to impossible dir. spec.) */
    static char lastdir[FILNAMSIZ] = "\t";

    static char *pathptr = pathbuf;     /* For debugger */
    static char *devptr, *dirptr;
    static int  devlen, dirlen;
    static int  root_dirlen;
    static char *end;
    static int  first_comp, root_has_dir;
    static int  rootlen=0;
    static char *rootend;
    static int  mkdir_failed=0;
    int status;
    struct FAB fab;
    struct NAM_STRUCT nam;


/************
 *** ROOT ***
 ************/

#if (!defined(SFX) || defined(SFX_EXDIR))
    if (function == ROOT)
    {   /*  Assume VMS root spec */
        /* 2006-01-20 SMS.
           Changed to use sys$parse() instead of sys$filescan() for analysis
           of the user-specified destination directory.  Previously, various
           values behaved badly, without complaint, e.g. "-d sys$scratch".
        */
        char *root_dest;

        /* If the root path has already been set, return immediately. */
        if (rootlen > 0)
            return MPN_OK;

        /* Initialization. */
        root_dest = PATH_DEFAULT;   /* Default destination for ODSx sensing. */
        root_has_dir = 0;           /* Root includes a directory. */
        fab = cc$rms_fab;           /* Initialize FAB. */
        nam = CC_RMS_NAM;           /* Initialize NAM[L]. */
        fab.FAB_NAM = &nam;         /* Point FAB to NAM[L]. */

#ifdef NAML$C_MAXRSS

        fab.fab$l_dna = (char *) -1;    /* Using NAML for default name. */
        fab.fab$l_fna = (char *) -1;    /* Using NAML for file name. */

#endif /* NAML$C_MAXRSS */

        /* Specified file spec. */
        FAB_OR_NAML(fab, nam).FAB_OR_NAML_FNA = pathcomp;
        FAB_OR_NAML(fab, nam).FAB_OR_NAML_FNS = strlen(pathcomp);

        /* Default file spec. */
        FAB_OR_NAML(fab, nam).FAB_OR_NAML_DNA = PATH_DEFAULT;
        FAB_OR_NAML(fab, nam).FAB_OR_NAML_DNS = strlen(PATH_DEFAULT);

        /* Expanded file spec. */
        nam.NAM_ESA = pathbuf;
        nam.NAM_ESS = NAM_MAXRSS;

        status = sys$parse(&fab);

        /* OK so far, if OK or if directory not found. */
        if (((status & STS$M_SEVERITY) != STS$K_SUCCESS) &&
            (status != RMS$_DNF))
        {
            /* Invalid destination directory specified. */
            Info(slide, 1, ((char *)slide,
              "Invalid destination directory (parse error): %s\n",
              FnFilter1(pathcomp)));
            return MPN_ERR_SKIP;
        }

        /* Should be only a device:[directory], so name+type+version
           should have length 2 (".;").
        */
        if (nam.NAM_B_NAME + nam.NAM_B_TYPE + nam.NAM_B_VER > 2)
        {
            Info(slide, 1, ((char *)slide,
              "Invalid destination directory (includes file name): %s\n",
              FnFilter1(nam.NAM_ESA)));
            return MPN_ERR_SKIP;
        }

        /* Truncate at name, leaving only "dev:[dir]". */
        *nam.NAM_L_NAME = '\0';
        rootlen = nam.NAM_L_NAME - nam.NAM_ESA;

        /* Remove any trailing dots in directory. */
        if ((nam.NAM_ESA[rootlen-1] == ']') &&
            (nam.NAM_ESA[rootlen-2] != '^'))
        {
            root_has_dir = 1;
            rootlen -= 2;
            while ((nam.NAM_ESA[rootlen] == '.') &&
                   (nam.NAM_ESA[rootlen-1] != '^'))
            {
                rootlen--;
            }
            nam.NAM_ESA[++rootlen] = ']';
            nam.NAM_ESA[++rootlen] = '\0';
        }

        devlen = nam.NAM_L_DIR - nam.NAM_ESA;

        /* If directory not found, then create it. */
        if (status == RMS$_DNF)
        {
            if (status = mkdir(nam.NAM_ESA, 0))
            {
                Info(slide, 1, ((char *)slide,
                  "Can not create destination directory: %s\n",
                  FnFilter1(nam.NAM_ESA)));

                /* path didn't exist, tried to create, and failed. */
                return MPN_ERR_SKIP;
            }
        }

        /* Save the (valid) device:[directory] spec. */
        strcpy(pathbuf, nam.NAM_ESA);
        root_dest = pathbuf;

        /* At this point, the true destination is known.  If the user
           supplied an invalid destination directory, the default
           directory will be used.  (This may be pointless, but should
           be safe.)
        */

        /* If not yet known, determine the destination (root_dest) file
           system type (ODS2 or ODS5).
        */
        if (ods2_names < 0)
        {
            /* If user doesn't force ODS2, set flag according to dest. */
            if (uO.ods2_flag == 0)
            {
                ods2_names = (dest_struct_level(root_dest) <= DVI$C_ACP_F11V2);
            }
            else
            {
                ods2_names = 1;     /* User demands ODS2 names. */
            }
        }

        /* Replace trailing "]" with ".", for later appending. */
        if ((pathbuf[rootlen-1] == ']') || (pathbuf[rootlen-1] == '>'))
        {
            pathbuf[rootlen-1] = '.';
        }

        /* Set various pointers and lengths. */
        devptr = pathbuf;
        dirptr = pathbuf + (nam.NAM_L_DIR - nam.NAM_ESA);
        rootend = pathbuf + rootlen;
        *(end = rootend) = '\0';
        root_dirlen = dirlen = rootlen - devlen;
        first_comp = !root_has_dir;
        return MPN_OK;
    }
#endif /* !SFX || SFX_EXDIR */


/************
 *** INIT ***
 ************/

    if ( function == INIT )
    {
        if ( strlen(G.filename) + rootlen + 13 > NAM_MAXRSS )
            return MPN_ERR_TOOLONG;

        if ( rootlen == 0 )     /* No root given, reset everything. */
        {
            devptr = dirptr = rootend = pathbuf;
            devlen = dirlen = 0;
        }
        end = rootend;
        first_comp = !root_has_dir;
        if ( dirlen = root_dirlen )
            end[-1] = '.';
        *end = '\0';
        return MPN_OK;
    }


/******************
 *** APPEND_DIR ***
 ******************/
    if ( function == APPEND_DIR )
    {
        int cmplen;

        cmplen = strlen(pathcomp);

        if ( first_comp )
        {
            *end++ = '[';
            if ( cmplen )
                *end++ = '.';   /*       "dir/..." --> "[.dir...]"    */
            /*                     else  "/dir..." --> "[dir...]"     */
            first_comp = 0;
        }

        if ( cmplen == 1 && *pathcomp == '.' )
            ; /* "..././..." -- ignore */

        else if ( cmplen == 2 && pathcomp[0] == '.' && pathcomp[1] == '.' )
        {   /* ".../../..." -- convert to "...-..." */
            *end++ = '-';
            *end++ = '.';
        }

        else if ( cmplen + (end-pathptr) > NAM_MAXRSS )
            return MPN_ERR_TOOLONG;

        else
        {
            strcpy(end, pathcomp);
            *(end+=cmplen) = '.';
            ++end;
        }
        dirlen = end - dirptr;
        *end = '\0';
        return MPN_OK;
    }


/*******************
 *** APPEND_NAME ***
 *******************/
    if ( function == APPEND_NAME )
    {
        if ( fcn & USE_DEFAULT )
        {   /* Expand renamed filename using collected path, return
             *  at pathcomp */
            fab = cc$rms_fab;           /* Initialize FAB. */
            nam = CC_RMS_NAM;           /* Initialize NAM[L]. */
            fab.FAB_NAM = &nam;         /* Point FAB to NAM[L]. */

#ifdef NAML$C_MAXRSS

            fab.fab$l_dna = (char *) -1;    /* Using NAML for default name. */
            fab.fab$l_fna = (char *) -1;    /* Using NAML for file name. */

#endif /* NAML$C_MAXRSS */

            FAB_OR_NAML(fab, nam).FAB_OR_NAML_DNA = pathptr;
            FAB_OR_NAML(fab, nam).FAB_OR_NAML_DNS = end - pathptr;
            FAB_OR_NAML(fab, nam).FAB_OR_NAML_FNA = G.filename;
            FAB_OR_NAML(fab, nam).FAB_OR_NAML_FNS = strlen(G.filename);

            nam.NAM_ESA = pathcomp;     /* (Great design. ---v.  SMS.) */
            nam.NAM_ESS = NAM_MAXRSS;   /* Assume large enough. */

            if (!OK(status = sys$parse(&fab)) && status == RMS$_DNF )
                                         /* Directory not found: */
            {                            /* ... try to create it */
                char    save;
                char    *dirend;
                int     mkdir_failed;

                dirend = (char*)nam.NAM_L_DIR + nam.NAM_B_DIR;
                save = *dirend;
                *dirend = '\0';
                if ( (mkdir_failed = mkdir(nam.NAM_L_DEV, 0)) &&
                     errno == EEXIST )
                    mkdir_failed = 0;
                *dirend = save;
                if ( mkdir_failed )
                    return 3;
                created_dir = TRUE;
            }                                /* if (sys$parse... */
            pathcomp[nam.NAM_ESL] = '\0';
            return MPN_OK;
        }                                /* if (USE_DEFAULT) */
        else
        {
            *end = '\0';
            if ( dirlen )
            {
                dirptr[dirlen-1] = ']'; /* Close directory */

                /*
                 *  Try to create the target directory.
                 *  Don't waste time creating directory that was created
                 *  last time.
                 */
                if ( STRICMP(lastdir, pathbuf) )
                {
                    mkdir_failed = 0;
                    if ( mkdir(pathbuf, 0) )
                    {
                        if ( errno != EEXIST )
                            mkdir_failed = 1;   /* Mine for GETPATH */
                    }
                    else
                        created_dir = TRUE;
                    strcpy(lastdir, pathbuf);
                }
            }
            else
            {   /*
                 * Target directory unspecified.
                 * Try to create "SYS$DISK:[]"
                 */
                if ( strcmp(lastdir, PATH_DEFAULT) )
                {
                    strcpy(lastdir, PATH_DEFAULT);
                    mkdir_failed = 0;
                    if ( mkdir(lastdir, 0) && errno != EEXIST )
                        mkdir_failed = 1;   /* Mine for GETPATH */
                }
            }
            if ( strlen(pathcomp) + (end-pathbuf) > 255 )
                return MPN_INF_TRUNC;
            strcpy(end, pathcomp);
            end += strlen(pathcomp);
            return MPN_OK;
        }
    }


/***************
 *** GETPATH ***
 ***************/
    if ( function == GETPATH )
    {
        if ( mkdir_failed )
            return MPN_ERR_SKIP;
        *end = '\0';                    /* To be safe */
        strcpy( pathcomp, pathbuf );
        return MPN_OK;
    }


/***********
 *** END ***
 ***********/
    if ( function == END )
    {
        Trace((stderr, "checkdir(): nothing to free...\n"));
        rootlen = 0;
        return MPN_OK;
    }

    return MPN_INVALID; /* should never reach */

}



int check_for_newer(__G__ filenam)   /* return 1 if existing file newer or */
    __GDEF                           /*  equal; 0 if older; -1 if doesn't */
    char *filenam;                   /*  exist yet */
{
#ifdef USE_EF_UT_TIME
    iztimes z_utime;
    struct tm *t;
#endif
    char *filenam_stat;
    unsigned short timbuf[7];
    unsigned dy, mo, yr, hh, mm, ss, dy2, mo2, yr2, hh2, mm2, ss2;
    struct FAB fab;
    struct XABDAT xdat;
#ifdef NAML$C_MAXRSS
    struct NAM_STRUCT nam;
#endif

    /* 2008-07-12 SMS.
     * Special case for "." as a file name, not as the current directory.
     * Substitute ".;" to keep stat() from seeing a plain ".".
    */
    if (strcmp(filenam, ".") == 0)
        filenam_stat = ".;";
    else
        filenam_stat = filenam;

    if (stat(filenam_stat, &G.statbuf))
        return DOES_NOT_EXIST;

    fab  = cc$rms_fab;                  /* Initialize FAB. */
    xdat = cc$rms_xabdat;               /* Initialize XAB. */

#ifdef NAML$C_MAXRSS

    nam  = CC_RMS_NAM;                  /* Initialize NAM[L]. */
    fab.FAB_NAM = &nam;                 /* Point FAB to NAM[L]. */

    fab.fab$l_dna = (char *) -1;    /* Using NAML for default name. */
    fab.fab$l_fna = (char *) -1;    /* Using NAML for file name. */

#endif /* NAML$C_MAXRSS */

    FAB_OR_NAML(fab, nam).FAB_OR_NAML_FNA = filenam;
    FAB_OR_NAML(fab, nam).FAB_OR_NAML_FNS = strlen(filenam);

    fab.fab$l_xab = (char *) &xdat;
    fab.fab$l_fop = FAB$M_GET | FAB$M_UFO;

    if (ERR(sys$open(&fab)))             /* open failure:  report exists and */
        return EXISTS_AND_OLDER;         /*  older so new copy will be made  */
    sys$numtim(&timbuf, &xdat.xab$q_cdt);
    fab.fab$l_xab = NULL;

    sys$dassgn(fab.fab$l_stv);
    sys$close(&fab);   /* be sure file is closed and RMS knows about it */

#ifdef USE_EF_UT_TIME
    if (G.extra_field &&
#ifdef IZ_CHECK_TZ
        G.tz_is_valid &&
#endif
        (ef_scan_for_izux(G.extra_field, G.lrec.extra_field_length, 0,
                          G.lrec.last_mod_dos_datetime, &z_utime, NULL)
         & EB_UT_FL_MTIME))
        t = localtime(&(z_utime.mtime));
    else
        t = (struct tm *)NULL;

    if (t != (struct tm *)NULL)
    {
        yr2 = (unsigned)(t->tm_year) + 1900;
        mo2 = (unsigned)(t->tm_mon) + 1;
        dy2 = (unsigned)(t->tm_mday);
        hh2 = (unsigned)(t->tm_hour);
        mm2 = (unsigned)(t->tm_min);
        ss2 = (unsigned)(t->tm_sec);

        /* round to nearest sec--may become 60,
           but doesn't matter for compare */
        ss = (unsigned)((float)timbuf[5] + (float)timbuf[6]*.01 + 0.5);
        TTrace((stderr, "check_for_newer:  using Unix extra field mtime\n"));
    }
    else
#endif /* USE_EF_UT_TIME */
    {
        yr2 = ((G.lrec.last_mod_dos_datetime >> 25) & 0x7f) + 1980;
        mo2 = (G.lrec.last_mod_dos_datetime >> 21) & 0x0f;
        dy2 = (G.lrec.last_mod_dos_datetime >> 16) & 0x1f;
        hh2 = (G.lrec.last_mod_dos_datetime >> 11) & 0x1f;
        mm2 = (G.lrec.last_mod_dos_datetime >> 5) & 0x3f;
        ss2 = (G.lrec.last_mod_dos_datetime << 1) & 0x1f;

        /* round to nearest 2 secs--may become 60,
           but doesn't matter for compare */
        ss = (unsigned)((float)timbuf[5] + (float)timbuf[6]*.01 + 1.) & (~1);
    }
    yr = timbuf[0];
    mo = timbuf[1];
    dy = timbuf[2];
    hh = timbuf[3];
    mm = timbuf[4];

    if (yr > yr2)
        return EXISTS_AND_NEWER;
    else if (yr < yr2)
        return EXISTS_AND_OLDER;

    if (mo > mo2)
        return EXISTS_AND_NEWER;
    else if (mo < mo2)
        return EXISTS_AND_OLDER;

    if (dy > dy2)
        return EXISTS_AND_NEWER;
    else if (dy < dy2)
        return EXISTS_AND_OLDER;

    if (hh > hh2)
        return EXISTS_AND_NEWER;
    else if (hh < hh2)
        return EXISTS_AND_OLDER;

    if (mm > mm2)
        return EXISTS_AND_NEWER;
    else if (mm < mm2)
        return EXISTS_AND_OLDER;

    if (ss >= ss2)
        return EXISTS_AND_NEWER;

    return EXISTS_AND_OLDER;
}



#ifdef RETURN_CODES
void return_VMS(__G__ err)
    __GDEF
#else
void return_VMS(err)
#endif
    int err;
{
    int severity;

#ifdef RETURN_CODES
/*---------------------------------------------------------------------------
    Do our own, explicit processing of error codes and print message, since
    VMS misinterprets return codes as rather obnoxious system errors ("access
    violation," for example).
  ---------------------------------------------------------------------------*/

    switch (err) {
        case PK_COOL:
            break;   /* life is fine... */
        case PK_WARN:
            Info(slide, 1, ((char *)slide, "\n\
[return-code %d:  warning error \
(e.g., failed CRC or unknown compression method)]\n", err));
            break;
        case PK_ERR:
        case PK_BADERR:
            Info(slide, 1, ((char *)slide, "\n\
[return-code %d:  error in zipfile \
(e.g., cannot find local file header sig)]\n", err));
            break;
        case PK_MEM:
        case PK_MEM2:
        case PK_MEM3:
        case PK_MEM4:
        case PK_MEM5:
            Info(slide, 1, ((char *)slide,
              "\n[return-code %d:  insufficient memory]\n", err));
            break;
        case PK_NOZIP:
            Info(slide, 1, ((char *)slide,
              "\n[return-code %d:  zipfile not found]\n", err));
            break;
        case PK_PARAM:   /* exit(PK_PARAM); gives "access violation" */
            Info(slide, 1, ((char *)slide, "\n\
[return-code %d:  bad or illegal parameters specified on command line]\n",
              err));
            break;
        case PK_FIND:
            Info(slide, 1, ((char *)slide,
              "\n[return-code %d:  no files found to extract/view/etc.]\n",
              err));
            break;
        case PK_DISK:
            Info(slide, 1, ((char *)slide,
              "\n[return-code %d:  disk full or other I/O error]\n", err));
            break;
        case PK_EOF:
            Info(slide, 1, ((char *)slide, "\n\
[return-code %d:  unexpected EOF in zipfile (i.e., truncated)]\n", err));
            break;
        case IZ_CTRLC:
            Info(slide, 1, ((char *)slide,
              "\n[return-code %d:  you hit ctrl-C to terminate]\n", err));
            break;
        case IZ_UNSUP:
            Info(slide, 1, ((char *)slide, "\n\
[return-code %d:  unsupported compression or encryption for all files]\n",
              err));
            break;
        case IZ_BADPWD:
            Info(slide, 1, ((char *)slide,
              "\n[return-code %d:  bad decryption password for all files]\n",
              err));
            break;
#ifdef DO_SAFECHECK_2GB
        case IZ_ERRBF:
            Info(slide, 1, ((char *)slide,
              "\n[return-code %d:  big-file archive, small-file program]\n",
              err));
            break;
#endif /* DO_SAFECHECK_2GB */
        default:
            Info(slide, 1, ((char *)slide,
              "\n[return-code %d:  unknown return-code (screw-up)]\n", err));
            break;
    }
#endif /* RETURN_CODES */

/*---------------------------------------------------------------------------
 *  Return an intelligent status/severity level:
 *
 *  2007-01-29 SMS.
 *
 *  VMS Status Code Summary  (See STSDEF.H for details.)
 *
 *      Bits:   31:28    27:16     15:3     2 1 0
 *      Field:  Control  Facility  Message  Severity
 *                                          -----
 *                                          0 0 0  0    Warning
 *                                          0 0 1  1    Success
 *                                          0 1 0  2    Error
 *                                          0 1 1  3    Information
 *                                          1 0 0  4    Severe (fatal) error
 *
 *  In the Control field, bits 31:29 are reserved.  Bit 28 inhibits
 *  printing the message.  In the Facility field, bit 27 means
 *  customer-defined (not HP-assigned, like us).  In the Message field,
 *  bit 15 means facility-specific (which our messages are).
 *
 *  Note that the C library translates exit(0) to a $STATUS value of 1
 *  (i.e., exit is both silent and has a $SEVERITY of "success").
 *
 *  Previous versions of Info-ZIP programs used a generic ("chosen (by
 *  experimentation)") Control+Facility code of 0x7FFF, which included
 *  some reserved control bits, the inhibit-printing bit, and the
 *  customer-defined bit.
 *
 *  HP has now assigned official Facility names and corresponding
 *  Facility codes for the Info-ZIP products:
 *
 *      Facility Name    Facility Code
 *      IZ_UNZIP         1954 = 0x7A2
 *      IZ_ZIP           1955 = 0x7A3
 *
 *  Now, unless the CTL_FAC_IZ_UZP macro is defined at build-time, we
 *  will use the official Facility code.
 *
  ---------------------------------------------------------------------------*/

/* Official HP-assigned Info-ZIP UnZip Facility code. */
#define FAC_IZ_UZP 1954   /* 0x7A2 */

#ifndef CTL_FAC_IZ_UZP
   /*
    * Default is inhibit-printing with the official Facility code.
    */
#  define CTL_FAC_IZ_UZP ((0x1 << 12) | FAC_IZ_UZP)
#  define MSG_FAC_SPEC 0x8000   /* Facility-specific code. */
#else /* CTL_FAC_IZ_UZP */
   /* Use the user-supplied Control+Facility code for err or warn. */
#  ifndef MSG_FAC_SPEC          /* Old default is not Facility-specific. */
#    define MSG_FAC_SPEC 0x0    /* Facility-specific code.  Or 0x8000. */
#  endif /* !MSG_FAC_SPEC */
#endif /* ?CTL_FAC_IZ_ZIP */
#define VMS_UZ_FAC_BITS       ((CTL_FAC_IZ_UZP << 16) | MSG_FAC_SPEC)

    severity = (err == PK_WARN) ? 0 :                           /* warn  */
               (err == PK_ERR ||                                /* error */
                (err >= PK_NOZIP && err <= PK_FIND) ||          /*  ...  */
                (err >= IZ_CTRLC && err <= IZ_BADPWD)) ? 2 :    /*  ...  */
               4;                                               /* fatal */

    exit(                                           /* $SEVERITY:            */
         (err == PK_COOL) ? SS$_NORMAL :            /* success               */
         (VMS_UZ_FAC_BITS | (err << 4) | severity)  /* warning, error, fatal */
        );

} /* end function return_VMS() */


#ifdef MORE
static int scrnlines = -1;
static int scrncolumns = -1;
static int scrnwrap = -1;


static int getscreeninfo(int *tt_rows, int *tt_cols, int *tt_wrap)
{
    /*
     * For VMS v5.x:
     *   IO$_SENSEMODE/SETMODE info:  Programming, Vol. 7A, System Programming,
     *     I/O User's: Part I, sec. 8.4.1.1, 8.4.3, 8.4.5, 8.6
     *   sys$assign(), sys$qio() info:  Programming, Vol. 4B, System Services,
     *     System Services Reference Manual, pp. sys-23, sys-379
     *   fixed-length descriptor info:  Programming, Vol. 3, System Services,
     *     Intro to System Routines, sec. 2.9.2
     * GRR, 15 Aug 91 / SPC, 07 Aug 1995, 14 Nov 1999
     */

#ifndef OUTDEVICE_NAME
#define OUTDEVICE_NAME  "SYS$OUTPUT"
#endif

    static ZCONST struct dsc$descriptor_s OutDevDesc =
        {(sizeof(OUTDEVICE_NAME) - 1), DSC$K_DTYPE_T, DSC$K_CLASS_S,
         OUTDEVICE_NAME};
     /* {dsc$w_length, dsc$b_dtype, dsc$b_class, dsc$a_pointer}; */

    short  OutDevChan, iosb[4];
    long   status;
    struct tt_characts
    {
        uch class, type;
        ush pagewidth;
        union {
            struct {
                uch ttcharsbits[3];
                uch pagelength;
            } ttdef_bits;
            unsigned ttcharflags;
        } ttdef_area;
    }      ttmode;              /* total length = 8 bytes */


    /* assign a channel to standard output */
    status = sys$assign(&OutDevDesc, &OutDevChan, 0, 0);
    if (OK(status))
    {
        /* use sys$qiow and the IO$_SENSEMODE function to determine
         * the current tty status.
         */
        status = sys$qiow(0, OutDevChan, IO$_SENSEMODE, &iosb, 0, 0,
                          &ttmode, sizeof(ttmode), 0, 0, 0, 0);
        /* deassign the output channel by way of clean-up */
        (void) sys$dassgn(OutDevChan);
    }

    if ( OK(status) && OK(status = iosb[0]) ) {
        if (tt_rows != NULL)
            *tt_rows = ( (ttmode.ttdef_area.ttdef_bits.pagelength >= 5)
                        ? (int) (ttmode.ttdef_area.ttdef_bits.pagelength)
                                                        /* TT device value */
                        : (24) );                       /* VT 100 default  */
        if (tt_cols != NULL)
            *tt_cols = ( (ttmode.pagewidth >= 10)
                        ? (int) (ttmode.pagewidth)      /* TT device value */
                        : (80) );                       /* VT 100 default  */
        if (tt_wrap != NULL)
            *tt_wrap = ((ttmode.ttdef_area.ttcharflags & TT$M_WRAP) != 0);
    } else {
        /* VT 100 defaults */
        if (tt_rows != NULL)
            *tt_rows = 24;
        if (tt_cols != NULL)
            *tt_cols = 80;
        if (tt_wrap != NULL)
            *tt_wrap = FALSE;
    }

    return (OK(status));
}

int screensize(int *tt_rows, int *tt_cols)
{
    if (scrnlines < 0 || scrncolumns < 0)
        getscreeninfo(&scrnlines, &scrncolumns, &scrnwrap);
    if (tt_rows != NULL) *tt_rows = scrnlines;
    if (tt_cols != NULL) *tt_cols = scrncolumns;
    return !(scrnlines > 0 && scrncolumns > 0);
}

int screenlinewrap()
{
    if (scrnwrap == -1)
        getscreeninfo(&scrnlines, &scrncolumns, &scrnwrap);
    return (scrnwrap);
}
#endif /* MORE */


#ifndef SFX

/************************/
/*  Function version()  */
/************************/

/* 2004-11-23 SMS.
 * Changed to include the "-x" part of the VMS version.
 * Added the IA64 system type name.
 * Prepared for VMS versions after 9.  (We should live so long.)
 */

void version(__G)
    __GDEF
{
    int len;
#ifdef VMS_VERSION
    char *chrp1;
    char *chrp2;
    char buf[40];
    char vms_vers[16];
    int ver_maj;
#endif
#ifdef __DECC_VER
    char buf2[40];
    int  vtyp;
#endif

#ifdef VMS_VERSION
    /* Truncate the version string at the first (trailing) space. */
    strncpy(vms_vers, VMS_VERSION, sizeof(vms_vers));
    vms_vers[sizeof(vms_vers)-1] = '\0';
    chrp1 = strchr(vms_vers, ' ');
    if (chrp1 != NULL)
        *chrp1 = '\0';

    /* Determine the major version number. */
    ver_maj = 0;
    chrp1 = strchr(&vms_vers[1], '.');
    for (chrp2 = &vms_vers[1];
         chrp2 < chrp1;
         ver_maj = ver_maj * 10 + *(chrp2++) - '0');
#endif /* VMS_VERSION */

/*  DEC C in ANSI mode does not like "#ifdef MACRO" inside another
    macro when MACRO is equated to a value (by "#define MACRO 1").   */

    len = sprintf((char *)slide, LoadFarString(CompiledWith),

#ifdef __GNUC__
      "gcc ", __VERSION__,
#else
#  if defined(DECC) || defined(__DECC) || defined (__DECC__)
      "DEC C",
#    ifdef __DECC_VER
      (sprintf(buf2, " %c%d.%d-%03d",
               ((vtyp = (__DECC_VER / 10000) % 10) == 6 ? 'T' :
                (vtyp == 8 ? 'S' : 'V')),
               __DECC_VER / 10000000,
               (__DECC_VER % 10000000) / 100000, __DECC_VER % 1000), buf2),
#    else
      "",
#    endif
#  else
#    ifdef VAXC
      "VAX C", "",
#    else
      "unknown compiler", "",
#    endif
#  endif
#endif

#ifdef VMS_VERSION
#  if defined(__alpha)
      "OpenVMS",
      (sprintf(buf, " (%s Alpha)", vms_vers), buf),
#  elif defined(__ia64)
      "OpenVMS",
      (sprintf(buf, " (%s IA64)", vms_vers), buf),
#  else /* VAX */
      (ver_maj >= 6) ? "OpenVMS" : "VMS",
      (sprintf(buf, " (%s VAX)", vms_vers), buf),
#  endif
#else
      "VMS",
      "",
#endif /* ?VMS_VERSION */

#ifdef __DATE__
      " on ", __DATE__
#else
      "", ""
#endif
    );

    (*G.message)((zvoid *)&G, slide, (ulg)len, 0);

} /* end function version() */

#endif /* !SFX */



#ifdef __DECC

/* 2004-11-20 SMS.
 *
 *       acc_cb(), access callback function for DEC C open().
 *
 *    Set some RMS FAB/RAB items, with consideration of user-specified
 * values from (DCL) SET RMS_DEFAULT.  Items of particular interest are:
 *
 *       fab$w_deq         default extension quantity (blocks) (write).
 *       rab$b_mbc         multi-block count.
 *       rab$b_mbf         multi-buffer count (used with rah and wbh).
 *
 *    See also the OPEN* macros in VMSCFG.H.  Currently, no notice is
 * taken of the caller-ID value, but options could be set differently
 * for read versus write access.  (I assume that specifying fab$w_deq,
 * for example, for a read-only file has no ill effects.)
 */

/* Global storage. */

int openr_id = OPENR_ID;        /* Callback id storage, read. */

/* acc_cb() */

int acc_cb(int *id_arg, struct FAB *fab, struct RAB *rab)
{
    int sts;

    /* Get process RMS_DEFAULT values, if not already done. */
    if (rms_defaults_known == 0)
    {
        get_rms_defaults();
    }

    /* If RMS_DEFAULT (and adjusted active) values are available, then set
     * the FAB/RAB parameters.  If RMS_DEFAULT values are not available,
     * suffer with the default parameters.
     */
    if (rms_defaults_known > 0)
    {
        /* Set the FAB/RAB parameters accordingly. */
        fab-> fab$w_deq = rms_ext_active;
        rab-> rab$b_mbc = rms_mbc_active;
        rab-> rab$b_mbf = rms_mbf_active;

        /* Truncate at EOF on close, as we'll probably over-extend. */
        fab-> fab$v_tef = 1;

        /* If using multiple buffers, enable read-ahead and write-behind. */
        if (rms_mbf_active > 1)
        {
            rab-> rab$v_rah = 1;
            rab-> rab$v_wbh = 1;
        }

        if (DIAG_FLAG)
        {
            fprintf(stderr,
              "Open callback.  ID = %d, deq = %6d, mbc = %3d, mbf = %3d.\n",
              *id_arg, fab-> fab$w_deq, rab-> rab$b_mbc, rab-> rab$b_mbf);
        }
    }

    /* Declare success. */
    return 0;
}



/*
 * 2004-09-19 SMS.
 *
 *----------------------------------------------------------------------
 *
 *       decc_init()
 *
 *    On non-VAX systems, uses LIB$INITIALIZE to set a collection of C
 *    RTL features without using the DECC$* logical name method.
 *
 *----------------------------------------------------------------------
 */

#ifdef __CRTL_VER
#if !defined(__VAX) && (__CRTL_VER >= 70301000)

#include <unixlib.h>

/*--------------------------------------------------------------------*/

/* Global storage. */

/*    Flag to sense if decc_init() was called. */

static int decc_init_done = -1;

/*--------------------------------------------------------------------*/

/* decc_init()

      Uses LIB$INITIALIZE to set a collection of C RTL features without
      requiring the user to define the corresponding logical names.
*/

/* Structure to hold a DECC$* feature name and its desired value. */

typedef struct
{
   char *name;
   int value;
} decc_feat_t;

/* Array of DECC$* feature names and their desired values. */

decc_feat_t decc_feat_array[] = {

   /* Preserve command-line case with SET PROCESS/PARSE_STYLE=EXTENDED */
 { "DECC$ARGV_PARSE_STYLE", 1 },

   /* Preserve case for file names on ODS5 disks. */
 { "DECC$EFS_CASE_PRESERVE", 1 },

   /* Enable multiple dots (and most characters) in ODS5 file names,
      while preserving VMS-ness of ";version". */
 { "DECC$EFS_CHARSET", 1 },

   /* List terminator. */
 { (char *)NULL, 0 } };


/* LIB$INITIALIZE initialization function. */

static void decc_init(void)
{
    int feat_index;
    int feat_value;
    int feat_value_max;
    int feat_value_min;
    int i;
    int sts;

    /* Set the global flag to indicate that LIB$INITIALIZE worked. */

    decc_init_done = 1;

    /* Loop through all items in the decc_feat_array[]. */

    for (i = 0; decc_feat_array[i].name != NULL; i++)
    {
        /* Get the feature index. */
        feat_index = decc$feature_get_index(decc_feat_array[i].name);
        if (feat_index >= 0)
        {
            /* Valid item.  Collect its properties. */
            feat_value = decc$feature_get_value(feat_index, 1);
            feat_value_min = decc$feature_get_value(feat_index, 2);
            feat_value_max = decc$feature_get_value(feat_index, 3);

            if ((decc_feat_array[i].value >= feat_value_min) &&
                (decc_feat_array[i].value <= feat_value_max))
            {
                /* Valid value.  Set it if necessary. */
                if (feat_value != decc_feat_array[i].value)
                {
                    sts = decc$feature_set_value(
                              feat_index,
                              1,
                              decc_feat_array[i].value);
                }
            }
            else
            {
                /* Invalid DECC feature value. */
                printf(" INVALID DECC FEATURE VALUE, %d: %d <= %s <= %d.\n",
                  feat_value,
                  feat_value_min, decc_feat_array[i].name, feat_value_max);
            }
        }
        else
        {
            /* Invalid DECC feature name. */
            printf(" UNKNOWN DECC FEATURE: %s.\n", decc_feat_array[i].name);
        }
    }
}

/* Get "decc_init()" into a valid, loaded LIB$INITIALIZE PSECT. */

#pragma nostandard

/* Establish the LIB$INITIALIZE PSECT, with proper alignment and
   attributes.
*/
globaldef {"LIB$INITIALIZ"} readonly _align (LONGWORD)
   int spare[8] = { 0 };
globaldef {"LIB$INITIALIZE"} readonly _align (LONGWORD)
   void (*x_decc_init)() = decc_init;

/* Fake reference to ensure loading the LIB$INITIALIZE PSECT. */

#pragma extern_model save
/* The declaration for LIB$INITIALIZE() is missing in the VMS system header
   files.  Addionally, the lowercase name "lib$initialize" is defined as a
   macro, so that this system routine can be reference in code using the
   traditional C-style lowercase convention of function names for readability.
   (VMS system functions declared in the VMS system headers are defined in a
   similar way to allow using lowercase names within the C code, whereas the
   "externally" visible names in the created object files are uppercase.)
 */
#ifndef lib$initialize
#  define lib$initialize LIB$INITIALIZE
#endif
int lib$initialize(void);
#pragma extern_model strict_refdef
int dmy_lib$initialize = (int)lib$initialize;
#pragma extern_model restore

#pragma standard

#endif /* !defined(__VAX) && (__CRTL_VER >= 70301000) */
#endif /* __CRTL_VER */
#endif /* __DECC */

#endif /* VMS */
