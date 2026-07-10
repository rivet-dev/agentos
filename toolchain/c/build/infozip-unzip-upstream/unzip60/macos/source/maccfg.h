/*
  Copyright (c) 1990-2000 Info-ZIP.  All rights reserved.

  See the accompanying file LICENSE, version 2000-Apr-09 or later
  (the contents of which are also included in unzip.h) for terms of use.
  If, for some reason, all these files are missing, the Info-ZIP license
  also may be found at:  ftp://ftp.info-zip.org/pub/infozip/license.html
*/
/*---------------------------------------------------------------------------
    MACOS specific configuration and declarations:
  ---------------------------------------------------------------------------*/

#ifndef __maccfg_h
#define __maccfg_h


/*****************************************************************************/
/*  Macros, missing RTL declarations, compatibility typedefs                 */
/*****************************************************************************/


#if defined(__MWERKS__) && defined(macintosh)
#  include <OSUtils.h>

   typedef unsigned long mode_t;
#  define _STAT

#  if (defined(MacStaticLib) || defined(USE_SIOUX))
#    define MAIN   UZ_EXP UzpMain   /* was UzpUnzip */
#  else
#    define MAIN   _dummy_main
#  endif
#endif

#ifdef THINK_C
#  ifndef __STDC__              /* if Think C hasn't defined __STDC__ ... */
#    define __STDC__ 1          /*   make sure it's defined: it needs it */
#  else
#    if !__STDC__               /* sometimes __STDC__ is defined as 0; */
#      undef __STDC__           /*   it needs to be 1 or required header */
#      define __STDC__ 1        /*   files are not properly included. */
#    endif /* !__STDC__ */
#  endif
#  define IOCompletionUPP   ProcPtr
#  define CREATOR  'KAHL'
#  define MAIN     _dummy_main
#endif /* THINK_C */

#ifdef MPW
#  include <Errors.h>
#  include <Files.h>
#  include <Memory.h>
#  include <Quickdraw.h>
#  include <ToolUtils.h>
#  ifndef QUERY_TRNEWLN
#    define QUERY_TRNEWLN
#  endif
#  ifdef fileno
#    undef fileno
#  endif
#  ifdef MCH_MACINTOSH
#    define CREATOR     'Manx'
#  else
#    define CREATOR     'MPS '
#  endif
#endif /* MPW */

#include <fcntl.h>              /* O_BINARY for open() w/o CR/LF translation */
#define fileno(x)       ((x) == stdout ? 1 : ((x) == stderr ? 2 : (short)(x)))
#define open            macopen
#define close           macclose
#define fclose(x)       macclose(fileno((x)))
#define read            macread
#define write           macwrite
#define lseek           maclseek
#define creat(x,y)      maccreat((x))
#define stat            UZmacstat
#define lstat           UZmacstat
#define dup
#ifndef MCH_MACINTOSH
#  define NO_STRNICMP
#endif

#define DIR_END ':'
#ifndef DATE_FORMAT
#  define DATE_FORMAT DF_MDY
#endif
#define lenEOL        1
#define PutNativeEOL  *q++ = native(CR);
#define NOANSIFILT  /* MWRKS C creates broken code for the ANSI-ESC filter */
#define MALLOC_WORK
#define INT_SPRINTF

#if (!defined(NO_EF_UT_TIME) && !defined(USE_EF_UT_TIME))
#  define USE_EF_UT_TIME
#endif

#undef IZ_CHECK_TZ
#undef MORE
#undef DYNAMIC_CRC_TABLE


#ifndef MPW
#  ifndef MacStaticLib
#    ifndef USE_SIOUX
#      define fgets       macfgets
#      define fflush(f)
#      define fprintf     macfprintf
#      define fputs(s,f)  macfprintf((f), "%s", (s))
#      define printf      macprintf
#      ifdef putc
#        undef putc
#      endif
#      define putc(c,f)   macfprintf((f), "%c", (c))
#    endif /* !USE_SIOUX */
#  else
#    ifdef putc
#      undef putc
#    endif
#    define putc(c,f)   fprintf((f), "%c", (c))
#  endif

#  define isatty(f)     (((f) >= 0) || ((f) <= 2))
#endif

#ifndef isascii
#  define isascii(c)    ((unsigned char)(c) <= 0x3F)
#endif

#include "macstat.h"
#include "macdir.h"

#ifdef CR
#  undef  CR
#endif


#ifdef foreign
#  undef foreign
#endif
#define foreign(c) ((c) & 0x80 ? MacRoman_to_WinCP1252[(c) & 0x7f] : (c))

#ifdef native
#  undef native
#endif
#define native(c)  ((c) & 0x80 ? WinCP1252_to_MacRoman[(c) & 0x7f] : (c))
#define NATIVE "MacRoman charset"

#ifdef _ISO_INTERN
#  undef _ISO_INTERN
#endif
#define _ISO_INTERN(str1) {register uch *p;\
   for (p=(uch *)(str1); *p; p++)\
     *p = (*p & 0x80) ? WinCP1252_to_MacRoman[*p & 0x7f] : *p;}

#ifdef _OEM_INTERN
#  undef _OEM_INTERN
#endif
#ifndef IZ_OEM2ISO_ARRAY
#  define IZ_OEM2ISO_ARRAY
#endif
#define _OEM_INTERN(str1) {register uch *p;\
   for (p=(uch *)(str1); *p; p++)\
     *p = (*p & 0x80) ? WinCP1252_to_MacRoman[oem2iso[*p & 0x7f]] : *p;}

#ifdef __FILEIO_C       /* get the ISO8859-1 <--> MacROMAN conversion tables */
#  include "charmap.h"
#else
   extern ZCONST uch WinCP1252_to_MacRoman[128];
   extern ZCONST uch MacRoman_to_WinCP1252[128];
#endif


#define EB_M3_FL_NOCHANGE   0x02    /* filename will be not changed */
/* other EB_M3 flags are defined in unzpriv.h */
#define EB_MAC3_SIZE        (EB_HEADSIZE + EB_MAC3_HLEN)

/*****************************************************************************/
/*  Structure typedefs                                                       */
/*****************************************************************************/

typedef struct _ZipExtraHdr {
        ush header;               /*    2 bytes */
        ush data;                 /*    2 bytes */
} ZIP_EXTRA_HEADER;


/* the following structure is a combination of the old and the new
   extra-field definition; so it's *not* the definition of the extra-field  */

typedef struct _MacInfo {
    unsigned char *filename;  /* for ZipIt ef */
    ush         header;     /* tag for this extra block type  */
    ush         data;       /* total data size for this block */
    ulg         size;       /* uncompressed finder attribute data size */

    ush         flags;      /* info bits:
                                 bit 0   if set, file is a data fork
                                 bit 1   if set, filename will be not changed
                                 bit 2   if set, Attribs is uncompressed
                                 bit 3   if set, date and times are in 64 bit;
                                         if zero, date and times are in 32 bit
                                 bit 4   if set, "local time - UTC" offsets are
                                         omitted
                                 bits 5-15       reserved; not tested;   */

    ush         CompType;
    ulg         CRCvalue;

    CInfoPBRec  fpb;        /* Macintosh FInfo / FXInfo structure */
    long        Cr_UTCoffs; /* difference "local time - UTC" for Creat-time */
    long        Md_UTCoffs; /* difference "local time - UTC" for Modif-time */
    long        Bk_UTCoffs; /* difference "local time - UTC" for Bckup-time */

    short       TextEncodingBase;   /* TextEncodingBase (Charset) */
    char       *FullPath;           /* Path of the current file */
    char       *FinderComment;      /* Finder Comment of current file */
} MACINFO;


/*****************************************************************************/
/*  Global Vars                                                              */
/*****************************************************************************/

#define SYSTEM_SPECIFIC_GLOBALS \
    short    giCursor;\
    CursHandle rghCursor[4];       /* status cursors */

#define SYSTEM_SPECIFIC_CTOR    MacGlobalsInit


/*****************************************************************************/
/*  Prototypes                                                               */
/*****************************************************************************/

void    screenOpen      OF((char *));                         /* macscreen.c */
void    screenControl   OF((char *, int));                    /* macscreen.c */
void    screenDump      OF((char *, long));                   /* macscreen.c */
void    screenUpdate    OF((WindowPtr));                      /* macscreen.c */
void    screenClose     OF((void));                           /* macscreen.c */
int     macgetch        OF((void));                           /* macscreen.c */

int     macmkdir        OF((char *));                             /* macos.c */
short   macopen         OF((char *, short));                      /* macos.c */
short   maccreat        OF((char *));                             /* macos.c */
short   macread         OF((short, char *, unsigned));            /* macos.c */
long    macwrite        OF((short, char *, unsigned));            /* macos.c */
short   macclose        OF((short));                              /* macos.c */
long    maclseek        OF((short, long, short));                 /* macos.c */
char   *macfgets        OF((char *, int, FILE *));                /* macos.c */
int     macfprintf      OF((FILE *, char *, ...));                /* macos.c */
int     macprintf       OF((char *, ...));                        /* macos.c */

ulg    makePPClong(ZCONST uch *sig);
ush    makePPCword(ZCONST uch *b);
void   UserStop(void);

#endif /* !__maccfg_h */
