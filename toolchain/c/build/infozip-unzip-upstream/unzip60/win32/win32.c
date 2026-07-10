/*
  Copyright (c) 1990-2008 Info-ZIP.  All rights reserved.

  See the accompanying file LICENSE, version 2007-Mar-04 or later
  (the contents of which are also included in unzip.h) for terms of use.
  If, for some reason, all these files are missing, the Info-ZIP license
  also may be found at:  ftp://ftp.info-zip.org/pub/infozip/license.html
*/
/*---------------------------------------------------------------------------

  win32.c

  32-bit Windows-specific (NT/9x) routines for use with Info-ZIP's UnZip 5.3
  and later.

  Contains:  GetLoadPath()
             Opendir()
             Readdir()
             Closedir()
             SetSD()              set security descriptor on file
             FindSDExtraField()   extract SD e.f. block from extra field
             IsWinNT()            indicate type of WIN32 platform
             test_NTSD()          test integrity of NT security data
             utime2NtfsFileTime()
             utime2VFatFileTime()
             FStampIsLocTime()
             NtfsFileTime2utime()
             VFatFileTime2utime()
             getNTfiletime()
             SetFileSize()
             close_outfile()
             defer_dir_attribs()
             set_direc_attribs()
             stamp_file()
             isfloppy()
             NTQueryVolInfo()
             IsVolumeOldFAT()
             do_wild()
             mapattr()
             mapname()
             maskDOSdevice()
             map2fat()
             checkdir()
             dateformat()
             dateseparator()
             version()
             screensize()
             zstat_win32()
             conv_to_rule()
             GetPlatformLocalTimezone()
             getch_win32()

  ---------------------------------------------------------------------------*/


#define UNZIP_INTERNAL
#include "../unzip.h"
#include <windows.h>    /* must be AFTER unzip.h to avoid struct G problems */
#ifdef __RSXNT__
#  include "../win32/rsxntwin.h"
#endif
#include "../win32/nt.h"

#ifndef FUNZIP          /* most of this file is not used with fUnZip */

/* some non-MS runtime headers (e.g. lcc) may miss this definition */
#ifndef FILE_WRITE_ATTRIBUTES
#  define FILE_WRITE_ATTRIBUTES 0x0100
#endif

#if (defined(__EMX__) || defined(__CYGWIN__))
#  define MKDIR(path,mode)   mkdir(path,mode)
#else
#  define MKDIR(path,mode)   mkdir(path)
#endif

#ifdef HAVE_WORKING_DIRENT_H
#  undef HAVE_WORKING_DIRENT_H
#endif
/* The emxrtl dirent support of (__GO32__ || __EMX__) converts to lowercase! */
#if defined(__CYGWIN__)
#  define HAVE_WORKING_DIRENT_H
#endif

#ifndef SFX
#  ifdef HAVE_WORKING_DIRENT_H
#    include <dirent.h>         /* use readdir() */
#    define zdirent  dirent
#    define zDIR     DIR
#    define Opendir  opendir
#    define Readdir  readdir
#    define Closedir closedir
#  else /* !HAVE_WORKING_DIRENT_H */
     typedef struct zdirent {
         char    reserved [21];
         char    ff_attrib;
         short   ff_ftime;
         short   ff_fdate;
         long    size;
         char    d_name[MAX_PATH];
         int     d_first;
         HANDLE  d_hFindFile;
     } zDIR;

     static zDIR           *Opendir  (const char *n);
     static struct zdirent *Readdir  (zDIR *d);
     static void            Closedir (zDIR *d);
#  endif /* ?HAVE_WORKING_DIRENT_H */
#endif /* !SFX */

#ifdef SET_DIR_ATTRIB
typedef struct NTdirattr {      /* struct for holding unix style directory */
    struct NTdirattr *next;     /*  info until can be sorted and set at end */
    char *fn;                   /* filename of directory */
    FILETIME Modft;    /* File time type defined in NT, `last modified' time */
    FILETIME Accft;    /* NT file time type, `last access' time */
    FILETIME Creft;    /* NT file time type, `file creation' time */
    int gotTime;
    unsigned perms;             /* same as min_info.file_attr */
#ifdef NTSD_EAS
    unsigned SDlen;             /* length of SD data in buf */
#endif
    char buf[1];                /* buffer stub for directory SD and name */
} NTdirattr;
#define NtAtt(d)  ((NTdirattr *)d)    /* typecast shortcut */
#endif /* SET_DIR_ATTRIB */


/* Function prototypes */
#ifdef NTSD_EAS
   static int  SetSD(__GPRO__ char *path, unsigned fperms,
                     uch *eb_ptr, unsigned eb_len);
   static int  FindSDExtraField(__GPRO__
                                uch *ef_ptr, unsigned ef_len,
                                uch **p_ebSD_ptr, unsigned *p_ebSD_len);
#endif

#ifndef NO_W32TIMES_IZFIX
   static void utime2NtfsFileTime(time_t ut, FILETIME *pft);
#endif
static void utime2VFatFileTime(time_t ut, FILETIME *pft, int clipDosMin);
#if (defined(W32_STAT_BANDAID) && !defined(NO_W32TIMES_IZFIX))
   static int NtfsFileTime2utime(const FILETIME *pft, time_t *ut);
#endif
#ifdef W32_STAT_BANDAID
   static int VFatFileTime2utime(const FILETIME *pft, time_t *ut);
#endif
static int FStampIsLocTime(__GPRO__ const char *path);


static int  getNTfiletime   (__GPRO__ FILETIME *pModFT, FILETIME *pAccFT,
                             FILETIME *pCreFT);
static int  isfloppy        (int nDrive);
static int  NTQueryVolInfo  (__GPRO__ const char *name);
static int  IsVolumeOldFAT  (__GPRO__ const char *name);
static void maskDOSdevice   (__GPRO__ char *pathcomp);
static void map2fat         (char *pathcomp, char **pEndFAT);


#if (defined(__MINGW32__) && !defined(USE_MINGW_GLOBBING))
   int _CRT_glob = 0;   /* suppress command line globbing by C RTL */
#endif

#ifdef ACORN_FTYPE_NFS
/* Acorn bits for NFS filetyping */
typedef struct {
  uch ID[2];
  uch size[2];
  uch ID_2[4];
  uch loadaddr[4];
  uch execaddr[4];
  uch attr[4];
} RO_extra_block;

#endif /* ACORN_FTYPE_NFS */

/* static int created_dir;      */     /* used by mapname(), checkdir() */
/* static int renamed_fullpath; */     /* ditto */
/* static int fnlen;            */     /* ditto */
/* static unsigned nLabelDrive; */     /* ditto */

extern char Far TruncNTSD[];    /* in extract.c */



#ifdef SFX

/**************************/
/* Function GetLoadPath() */
/**************************/

char *GetLoadPath(__GPRO)
{
#ifdef MSC
    extern char *_pgmptr;
    return _pgmptr;

#else    /* use generic API call */

    GetModuleFileName(NULL, G.filename, FILNAMSIZ);
    _ISO_INTERN(G.filename);    /* translate to codepage of C rtl's stdio */
    return G.filename;
#endif

} /* end function GetLoadPath() */





#else /* !SFX */

#ifndef HAVE_WORKING_DIRENT_H

/**********************/        /* Borrowed from ZIP 2.0 sources            */
/* Function Opendir() */        /* Difference: no special handling for      */
/**********************/        /*             hidden or system files.      */

static zDIR *Opendir(n)
    const char *n;          /* directory to open */
{
    zDIR *d;                /* malloc'd return value */
    char *p;                /* malloc'd temporary string */
    WIN32_FIND_DATAA fd;
    extent len = strlen(n);

    /* Start searching for files in directory n */

    if ((d = (zDIR *)malloc(sizeof(zDIR))) == NULL ||
        (p = malloc(strlen(n) + 5)) == NULL)
    {
        if (d != (zDIR *)NULL)
            free((void *)d);
        return (zDIR *)NULL;
    }
    INTERN_TO_ISO(n, p);
    if (len > 0) {
        if (p[len-1] == ':')
            p[len++] = '.';     /* x: => x:. */
        else if (p[len-1] == '/' || p[len-1] == '\\')
            --len;              /* foo/ => foo */
    }
    strcpy(p+len, "/*");

    if (INVALID_HANDLE_VALUE == (d->d_hFindFile = FindFirstFileA(p, &fd))) {
        free((zvoid *)d);
        free((zvoid *)p);
        return NULL;
    }
    strcpy(d->d_name, fd.cFileName);

    free((zvoid *)p);
    d->d_first = 1;
    return d;

} /* end of function Opendir() */




/**********************/        /* Borrowed from ZIP 2.0 sources            */
/* Function Readdir() */        /* Difference: no special handling for      */
/**********************/        /*             hidden or system files.      */

static struct zdirent *Readdir(d)
    zDIR *d;                    /* directory stream from which to read */
{
    /* Return pointer to first or next directory entry, or NULL if end. */

    if ( d->d_first )
        d->d_first = 0;
    else
    {
        WIN32_FIND_DATAA fd;

        if ( !FindNextFileA(d->d_hFindFile, &fd) )
            return NULL;

        ISO_TO_INTERN(fd.cFileName, d->d_name);
    }
    return (struct zdirent *)d;

} /* end of function Readdir() */




/***********************/
/* Function Closedir() */       /* Borrowed from ZIP 2.0 sources */
/***********************/

static void Closedir(d)
    zDIR *d;                    /* directory stream to close */
{
    FindClose(d->d_hFindFile);
    free(d);
}

#endif /* !HAVE_WORKING_DIRENT_H */
#endif /* ?SFX */




#ifdef NTSD_EAS

/**********************/
/*  Function SetSD()  */   /* return almost-PK errors */
/**********************/

static int SetSD(__G__ path, fperms, eb_ptr, eb_len)
    __GDEF
    char *path;
    unsigned fperms;
    uch *eb_ptr;
    unsigned eb_len;
{
    ulg ntsd_ucSize;
    VOLUMECAPS VolumeCaps;
    uch *security_data;
    int error;

    ntsd_ucSize = makelong(eb_ptr + (EB_HEADSIZE+EB_UCSIZE_P));
    if (ntsd_ucSize > 0L && eb_len <= (EB_NTSD_L_LEN + EB_CMPRHEADLEN))
        return IZ_EF_TRUNC;               /* no compressed data! */

    /* provide useful input */
    VolumeCaps.dwFileAttributes = fperms;
    VolumeCaps.bUsePrivileges = (uO.X_flag > 1);

    /* check target volume capabilities - just fall through
     * and try if fail */
    if (GetVolumeCaps(G.rootpath, path, &VolumeCaps) &&
        !(VolumeCaps.dwFileSystemFlags & FS_PERSISTENT_ACLS))
        return PK_OK;

    /* allocate storage for uncompressed data */
    security_data = (uch *)malloc((extent)ntsd_ucSize);
    if (security_data == (uch *)NULL)
        return PK_MEM4;

    error = memextract(__G__ security_data, ntsd_ucSize,
      (eb_ptr + (EB_HEADSIZE+EB_NTSD_L_LEN)), (ulg)(eb_len - EB_NTSD_L_LEN));

    if (error == PK_OK) {
        if (SecuritySet(path, &VolumeCaps, security_data)) {
            error = PK_COOL;
            if (!uO.tflag && QCOND2)
                Info(slide, 0, ((char *)slide, " (%ld bytes security)",
                  ntsd_ucSize));
        }
    }

    free(security_data);
    return error;
}




/********************************/   /* scan extra fields for something */
/*  Function FindSDExtraField() */   /*  we happen to know */
/********************************/
/* Returns TRUE when a valid NTFS SD block is found.
 * Address and size of the NTSD e.f. block are passed up to the caller.
 * In case of more than one valid NTSD block in the e.f., the last block
 * found is passed up.
 * Returns FALSE and leaves the content of the ebSD_ptr and ebSD_len
 * parameters untouched when no valid NTFS SD block is found. */
static int FindSDExtraField(__GPRO__
                            uch *ef_ptr, unsigned ef_len,
                            uch **p_ebSD_ptr, unsigned *p_ebSD_len)
{
    int rc = FALSE;

    if (!uO.X_flag)
        return FALSE;  /* user said don't process ACLs; for now, no other
                          extra block types are handled here */

    while (ef_len >= EB_HEADSIZE)
    {
        unsigned eb_id = makeword(EB_ID + ef_ptr);
        unsigned eb_len = makeword(EB_LEN + ef_ptr);

        if (eb_len > (ef_len - EB_HEADSIZE)) {
            /* discovered some extra field inconsistency! */
            Trace((stderr,
              "FindSDExtraField: block length %u > rest ef_size %u\n", eb_len,
              ef_len - EB_HEADSIZE));
            break;
        }

        switch (eb_id)
        {
            /* process security descriptor extra data if:
                 Caller is WinNT AND
                 Target local/remote drive supports acls AND
                 Target file is not a directory (else we defer processing
                   until later)
             */
            case EF_NTSD:
                if (!IsWinNT())
                    break; /* OS not capable of handling NTFS attributes */

                if (eb_len < EB_NTSD_L_LEN)
                    break; /* not a valid NTSD extra field */

                /* check if we know how to handle this version */
                if (*(ef_ptr + (EB_HEADSIZE+EB_NTSD_VERSION))
                    > (uch)EB_NTSD_MAX_VER)
                    break;

                *p_ebSD_ptr = ef_ptr;
                *p_ebSD_len = eb_len;
                rc = TRUE;
                break;

#ifdef DEBUG
            case EF_OS2:
            case EF_AV:
            case EF_PKVMS:
            case EF_PKW32:
            case EF_PKUNIX:
            case EF_IZVMS:
            case EF_IZUNIX:
            case EF_IZUNIX2:
            case EF_TIME:
            case EF_MAC3:
            case EF_JLMAC:
            case EF_ZIPIT:
            case EF_VMCMS:
            case EF_MVS:
            case EF_ACL:
            case EF_ATHEOS:
            case EF_BEOS:
            case EF_QDOS:
            case EF_AOSVS:
            case EF_SPARK:
            case EF_MD5:
            case EF_ASIUNIX:
                break;          /* shut up for other known e.f. blocks  */
#endif /* DEBUG */

            default:
                Trace((stderr,
                  "FindSDExtraField: unknown extra field block, ID=%u\n",
                  eb_id));
                break;
        }

        ef_ptr += (eb_len + EB_HEADSIZE);
        ef_len -= (eb_len + EB_HEADSIZE);
    }

    return rc;
}




#ifndef SFX

/**************************/
/*  Function test_NTSD()  */   /*  returns PK_WARN when NTSD data is invalid */
/**************************/

#ifdef __BORLANDC__
/* Turn off warning about not using all parameters for this function only */
#pragma argsused
#endif
int test_NTSD(__G__ eb, eb_size, eb_ucptr, eb_ucsize)
    __GDEF
    uch *eb;
    unsigned eb_size;
    uch *eb_ucptr;
    ulg eb_ucsize;
{
    return (ValidateSecurity(eb_ucptr) ? PK_OK : PK_WARN);
} /* end function test_NTSD() */

#endif /* !SFX */
#endif /* NTSD_EAS */




/**********************/
/* Function IsWinNT() */
/**********************/

int IsWinNT(void)       /* returns TRUE if real NT, FALSE if Win9x or Win32s */
{
    static DWORD g_PlatformId = 0xFFFFFFFF; /* saved platform indicator */

    if (g_PlatformId == 0xFFFFFFFF) {
        /* note: GetVersionEx() doesn't exist on WinNT 3.1 */
        if (GetVersion() < 0x80000000)
            g_PlatformId = TRUE;
        else
            g_PlatformId = FALSE;
    }
    return (int)g_PlatformId;
}


/* DEBUG_TIME insertion: */
#ifdef DEBUG_TIME
static int show_NTFileTime(FILE *hdo, char *TTmsg, int isloc, FILETIME *pft);

static int show_NTFileTime(FILE *hdo, char *TTmsg, int isloc, FILETIME *pft)
{
    SYSTEMTIME w32tm;
    int rval;

    rval = FileTimeToSystemTime(pft, &w32tm);
    if (!rval) {
        fprintf(hdo, "%s\n %08lX,%08lX (%s) -> Conversion failed !!!\n",
                TTmsg, (ulg)(pft->dwHighDateTime), (ulg)(pft->dwLowDateTime),
                (isloc ? "local" : "UTC"));
    } else {
        fprintf(hdo, "%s\n %08lx,%08lx -> %04u-%02u-%02u, %02u:%02u:%02u %s\n",
                TTmsg, (ulg)(pft->dwHighDateTime), (ulg)(pft->dwLowDateTime),
                w32tm.wYear, w32tm.wMonth, w32tm.wDay, w32tm.wHour,
                w32tm.wMinute, w32tm.wSecond, (isloc ? "local" : "UTC"));
    }
    return rval;
}
#define FTTrace(x)   show_NTFileTime x
#else
#define FTTrace(x)
#endif /* DEBUG_TIME */
/* end of DEBUG_TIME insertion */

#ifndef IZ_USE_INT64
#  if (defined(__GNUC__) || defined(ULONG_LONG_MAX))
     typedef long long            LLONG64;
     typedef unsigned long long   ULLNG64;
#    define IZ_USE_INT64
#  elif (defined(__WATCOMC__) && (__WATCOMC__ >= 1100))
     typedef __int64              LLONG64;
     typedef unsigned __int64     ULLNG64;
#    define IZ_USE_INT64
#  elif (defined(_MSC_VER) && (_MSC_VER >= 1100))
     typedef __int64              LLONG64;
     typedef unsigned __int64     ULLNG64;
#    define IZ_USE_INT64
#  elif (defined(__IBMC__) && (__IBMC__ >= 350))
     typedef __int64              LLONG64;
     typedef unsigned __int64     ULLNG64;
#    define IZ_USE_INT64
#  elif defined(HAVE_INT64)
     typedef __int64              LLONG64;
     typedef unsigned __int64     ULLNG64;
#    define IZ_USE_INT64
#  endif
#endif

/* scale factor and offset for conversion time_t -> FILETIME */
#define NT_QUANTA_PER_UNIX 10000000L
#define UNIX_TIME_ZERO_HI  0x019DB1DEUL
#define UNIX_TIME_ZERO_LO  0xD53E8000UL
/* special FILETIME values for bound-checks */
#define UNIX_TIME_UMAX_HI  0x0236485EUL
#define UNIX_TIME_UMAX_LO  0xD4A5E980UL
#define UNIX_TIME_SMIN_HI  0x0151669EUL
#define UNIX_TIME_SMIN_LO  0xD53E8000UL
#define UNIX_TIME_SMAX_HI  0x01E9FD1EUL
#define UNIX_TIME_SMAX_LO  0xD4A5E980UL
#define DOSTIME_MIN_FT_HI  0x01A8E79FUL
#define DOSTIME_MIN_FT_LO  0xE1D58000UL
/* time_t equivalent of DOSTIME_MINIMUM */
#define UTIME_1980_JAN_01_00_00   315532800L


#ifndef NO_W32TIMES_IZFIX
/*********************************/
/* Function utime2NtfsFileTime() */ /* convert Unix time_t format into the */
/*********************************/ /* form used by SetFileTime() in NT/9x */

static void utime2NtfsFileTime(time_t ut, FILETIME *pft)
{
#ifdef IZ_USE_INT64
    ULLNG64 NTtime;

    /* NT_QUANTA_PER_UNIX is small enough so that "ut * NT_QUANTA_PER_UNIX"
     * cannot overflow in 64-bit signed calculation, regardless whether "ut"
     * is signed or unsigned.  */
    NTtime = ((LLONG64)ut * NT_QUANTA_PER_UNIX) +
             ((ULLNG64)UNIX_TIME_ZERO_LO + ((ULLNG64)UNIX_TIME_ZERO_HI << 32));
    pft->dwLowDateTime = (DWORD)NTtime;
    pft->dwHighDateTime = (DWORD)(NTtime >> 32);

#else /* !IZ_USE_INT64 (64-bit integer arithmetics may not be supported) */
    unsigned int b1, b2, carry = 0;
    unsigned long r0, r1, r2, r3;
    long r4;            /* signed, to catch environments with signed time_t */

    b1 = ut & 0xFFFF;
    b2 = (ut >> 16) & 0xFFFF;       /* if ut is over 32 bits, too bad */
    r1 = b1 * (NT_QUANTA_PER_UNIX & 0xFFFF);
    r2 = b1 * (NT_QUANTA_PER_UNIX >> 16);
    r3 = b2 * (NT_QUANTA_PER_UNIX & 0xFFFF);
    r4 = b2 * (NT_QUANTA_PER_UNIX >> 16);
    r0 = (r1 + (r2 << 16)) & 0xFFFFFFFFL;
    if (r0 < r1)
        carry++;
    r1 = r0;
    r0 = (r0 + (r3 << 16)) & 0xFFFFFFFFL;
    if (r0 < r1)
        carry++;
    pft->dwLowDateTime = r0 + UNIX_TIME_ZERO_LO;
    if (pft->dwLowDateTime < r0)
        carry++;
    pft->dwHighDateTime = r4 + (r2 >> 16) + (r3 >> 16)
                            + UNIX_TIME_ZERO_HI + carry;
#endif /* ?IZ_USE_INT64 */

} /* end function utime2NtfsFileTime() */
#endif /* !NO_W32TIMES_IZFIX */



/*********************************/
/* Function utime2VFatFileTime() */ /* convert Unix time_t format into the */
/*********************************/ /* form used by SetFileTime() in NT/9x */

static void utime2VFatFileTime(time_t ut, FILETIME *pft, int clipDosMin)
{
    time_t utc = ut;
    struct tm *ltm;
    SYSTEMTIME w32tm;
    FILETIME lft;

    /* The milliseconds field gets always initialized to 0. */
    w32tm.wMilliseconds = 0;

#ifdef __BORLANDC__   /* Borland C++ 5.x crashes when trying to reference tm */
    if (utc < UTIME_1980_JAN_01_00_00)
        utc = UTIME_1980_JAN_01_00_00;
#endif
    ltm = localtime(&utc);
    if (ltm == (struct tm *)NULL)
        /* localtime() did not accept given utc time value; try to use
           the UTC value */
        ltm = gmtime(&utc);
    if (ltm == (struct tm *)NULL) {
        if (ut <= (UTIME_1980_JAN_01_00_00 + 86400)) {
            /* use DOSTIME_MINIMUM date instead of "early" failure dates */
            w32tm.wYear = 1980;
            w32tm.wMonth = 1;
            w32tm.wDay = 1;
            w32tm.wHour = 0;
            w32tm.wMinute = 0;
            w32tm.wSecond = 0;
        } else {
            /* as a last resort, use the current system time */
            GetLocalTime(&w32tm);
        }
    } else if (clipDosMin && (ltm->tm_year < 80)) {
        w32tm.wYear = 1980;
        w32tm.wMonth = 1;
        w32tm.wDay = 1;
        w32tm.wHour = 0;
        w32tm.wMinute = 0;
        w32tm.wSecond = 0;
    } else {
        w32tm.wYear = ltm->tm_year + 1900; /* year + 1900 -> year */
        w32tm.wMonth = ltm->tm_mon + 1;    /* 0..11 -> 1..12 */
        w32tm.wDay = ltm->tm_mday;         /* 1..31 */
        w32tm.wHour = ltm->tm_hour;        /* 0..23 */
        w32tm.wMinute = ltm->tm_min;       /* 0..59 */
        w32tm.wSecond = ltm->tm_sec;       /* 0..61 in ANSI C */
    }

    SystemTimeToFileTime(&w32tm, &lft);
    LocalFileTimeToFileTime(&lft, pft);

} /* end function utime2VFatFileTime() */



 /* nonzero if `y' is a leap year, else zero */
#define leap(y) (((y)%4 == 0 && (y)%100 != 0) || (y)%400 == 0)
 /* number of leap years from 1970 to `y' (not including `y' itself) */
#define nleap(y) (((y)-1969)/4 - ((y)-1901)/100 + ((y)-1601)/400)

extern ZCONST ush ydays[];              /* defined in fileio.c */

#if (defined(W32_STAT_BANDAID) && !defined(NO_W32TIMES_IZFIX))
/*********************************/
/* Function NtfsFileTime2utime() */
/*********************************/

static int NtfsFileTime2utime(const FILETIME *pft, time_t *ut)
{
#ifdef IZ_USE_INT64
    ULLNG64 NTtime;

    NTtime = ((ULLNG64)pft->dwLowDateTime +
              ((ULLNG64)pft->dwHighDateTime << 32));

#ifndef TIME_T_TYPE_DOUBLE
    /* underflow and overflow handling */
#ifdef CHECK_UTIME_SIGNED_UNSIGNED
    if ((time_t)0x80000000L < (time_t)0L)
    {
        if (NTtime < ((ULLNG64)UNIX_TIME_SMIN_LO +
                      ((ULLNG64)UNIX_TIME_SMIN_HI << 32))) {
            *ut = (time_t)LONG_MIN;
            return FALSE;
        }
        if (NTtime > ((ULLNG64)UNIX_TIME_SMAX_LO +
                      ((ULLNG64)UNIX_TIME_SMAX_HI << 32))) {
            *ut = (time_t)LONG_MAX;
            return FALSE;
        }
    }
    else
#endif /* CHECK_UTIME_SIGNED_UNSIGNED */
    {
        if (NTtime < ((ULLNG64)UNIX_TIME_ZERO_LO +
                      ((ULLNG64)UNIX_TIME_ZERO_HI << 32))) {
            *ut = (time_t)0;
            return FALSE;
        }
        if (NTtime > ((ULLNG64)UNIX_TIME_UMAX_LO +
                      ((ULLNG64)UNIX_TIME_UMAX_HI << 32))) {
            *ut = (time_t)ULONG_MAX;
            return FALSE;
        }
    }
#endif /* !TIME_T_TYPE_DOUBLE */

    NTtime -= ((ULLNG64)UNIX_TIME_ZERO_LO +
               ((ULLNG64)UNIX_TIME_ZERO_HI << 32));
    *ut = (time_t)(NTtime / (unsigned long)NT_QUANTA_PER_UNIX);
    return TRUE;
#else /* !IZ_USE_INT64 (64-bit integer arithmetics may not be supported) */
    time_t days;
    SYSTEMTIME w32tm;

#ifndef TIME_T_TYPE_DOUBLE
    /* underflow and overflow handling */
#ifdef CHECK_UTIME_SIGNED_UNSIGNED
    if ((time_t)0x80000000L < (time_t)0L)
    {
        if ((pft->dwHighDateTime < UNIX_TIME_SMIN_HI) ||
            ((pft->dwHighDateTime == UNIX_TIME_SMIN_HI) &&
             (pft->dwLowDateTime < UNIX_TIME_SMIN_LO))) {
            *ut = (time_t)LONG_MIN;
            return FALSE;
        if ((pft->dwHighDateTime > UNIX_TIME_SMAX_HI) ||
            ((pft->dwHighDateTime == UNIX_TIME_SMAX_HI) &&
             (pft->dwLowDateTime > UNIX_TIME_SMAX_LO))) {
            *ut = (time_t)LONG_MAX;
            return FALSE;
        }
    }
    else
#endif /* CHECK_UTIME_SIGNED_UNSIGNED */
    {
        if ((pft->dwHighDateTime < UNIX_TIME_ZERO_HI) ||
            ((pft->dwHighDateTime == UNIX_TIME_ZERO_HI) &&
             (pft->dwLowDateTime < UNIX_TIME_ZERO_LO))) {
            *ut = (time_t)0;
            return FALSE;
        }
        if ((pft->dwHighDateTime > UNIX_TIME_UMAX_HI) ||
            ((pft->dwHighDateTime == UNIX_TIME_UMAX_HI) &&
             (pft->dwLowDateTime > UNIX_TIME_UMAX_LO))) {
            *ut = (time_t)ULONG_MAX;
            return FALSE;
        }
    }
#endif /* !TIME_T_TYPE_DOUBLE */

    FileTimeToSystemTime(pft, &w32tm);

    /* set `days' to the number of days into the year */
    days = w32tm.wDay - 1 + ydays[w32tm.wMonth-1] +
           (w32tm.wMonth > 2 && leap (w32tm.wYear));

    /* now set `days' to the number of days since 1 Jan 1970 */
    days += 365 * (time_t)(w32tm.wYear - 1970) +
            (time_t)(nleap(w32tm.wYear));

    *ut = (time_t)(86400L * days + 3600L * (time_t)w32tm.wHour +
                   (time_t)(60 * w32tm.wMinute + w32tm.wSecond));
    return TRUE;
#endif /* ?IZ_USE_INT64 */
} /* end function NtfsFileTime2utime() */
#endif /* W32_STAT_BANDAID && !NO_W32TIMES_IZFIX */



#ifdef W32_STAT_BANDAID
/*********************************/
/* Function VFatFileTime2utime() */
/*********************************/

static int VFatFileTime2utime(const FILETIME *pft, time_t *ut)
{
    FILETIME lft;
#ifndef HAVE_MKTIME
    WORD wDOSDate, wDOSTime;
#else
    SYSTEMTIME w32tm;
    struct tm ltm;
#endif

    if (!FileTimeToLocalFileTime(pft, &lft)) {
        /* if pft cannot be converted to local time, set ut to current time */
        time(ut);
        return FALSE;
    }
    FTTrace((stdout, "VFatFT2utime, feed for mktime()", 1, &lft));
#ifndef HAVE_MKTIME
    /* This version of the FILETIME-to-UNIXTIME conversion function
     * uses DOS-DATE-TIME format as intermediate stage. For modification
     * and access times, this is no problem. But, the extra fine resolution
     * of the VFAT-stored creation time gets lost.
     */
    if (!FileTimeToDosDateTime(&lft, &wDOSDate, &wDOSTime)) {
        static const FILETIME dosmin_ft =
                {DOSTIME_MIN_FT_LO, DOSTIME_MIN_FT_HI};
        if (CompareFileTime(&lft, &dosmin_ft) <= 0) {
            /* underflow -> set to minimum DOS time */
            wDOSDate = (WORD)((DWORD)DOSTIME_MINIMUM >> 16);
            wDOSTime = (WORD)DOSTIME_MINIMUM;
        } else {
            /* overflow -> set to maximum DOS time */
            wDOSDate = (WORD)0xFF9F;    /* 2107-12-31 */
            wDOSTime = (WORD)0xBF7D;    /* 23:59:58 */
        }
    }
    TTrace((stdout,"DosDateTime is %04u-%02u-%02u %02u:%02u:%02u\n",
      (unsigned)((wDOSDate>>9)&0x7f)+1980,(unsigned)((wDOSDate>>5)&0x0f),
      (unsigned)(wDOSDate&0x1f),(unsigned)((wDOSTime>>11)&0x1f),
      (unsigned)((wDOSTime>>5)&0x3f),(unsigned)((wDOSTime<<1)&0x3e)));
    *ut = dos_to_unix_time(((ulg)wDOSDate << 16) | (ulg)wDOSTime);

    /* a cheap error check: dos_to_unix_time() only returns an odd time
     * when clipping at maximum time_t value. DOS_DATE_TIME values have
     * a resolution of 2 seconds and are therefore even numbers.
     */
    return (((*ut)&1) == (time_t)0);
#else /* HAVE_MKTIME */
    FileTimeToSystemTime(&lft, &w32tm);
#ifndef TIME_T_TYPE_DOUBLE
    /* underflow and overflow handling */
    /* TODO: The range checks are not accurate, the actual limits may
     *       be off by one daylight-saving-time shift (typically 1 hour),
     *       depending on the current state of "is_dst".
     */
#ifdef CHECK_UTIME_SIGNED_UNSIGNED
    if ((time_t)0x80000000L < (time_t)0L)
    {
        if ((pft->dwHighDateTime < UNIX_TIME_SMIN_HI) ||
            ((pft->dwHighDateTime == UNIX_TIME_SMIN_HI) &&
             (pft->dwLowDateTime < UNIX_TIME_SMIN_LO))) {
            *ut = (time_t)LONG_MIN;
            return FALSE;
        if ((pft->dwHighDateTime > UNIX_TIME_SMAX_HI) ||
            ((pft->dwHighDateTime == UNIX_TIME_SMAX_HI) &&
             (pft->dwLowDateTime > UNIX_TIME_SMAX_LO))) {
            *ut = (time_t)LONG_MAX;
            return FALSE;
        }
    }
    else
#endif /* CHECK_UTIME_SIGNED_UNSIGNED */
    {
        if ((pft->dwHighDateTime < UNIX_TIME_ZERO_HI) ||
            ((pft->dwHighDateTime == UNIX_TIME_ZERO_HI) &&
             (pft->dwLowDateTime < UNIX_TIME_ZERO_LO))) {
            *ut = (time_t)0;
            return FALSE;
        }
        if ((pft->dwHighDateTime > UNIX_TIME_UMAX_HI) ||
            ((pft->dwHighDateTime == UNIX_TIME_UMAX_HI) &&
             (pft->dwLowDateTime > UNIX_TIME_UMAX_LO))) {
            *ut = (time_t)ULONG_MAX;
            return FALSE;
        }
    }
#endif /* !TIME_T_TYPE_DOUBLE */
    ltm.tm_year = w32tm.wYear - 1900;
    ltm.tm_mon = w32tm.wMonth - 1;
    ltm.tm_mday = w32tm.wDay;
    ltm.tm_hour = w32tm.wHour;
    ltm.tm_min = w32tm.wMinute;
    ltm.tm_sec = w32tm.wSecond;
    ltm.tm_isdst = -1;  /* let mktime determine if DST is in effect */
    *ut = mktime(&ltm);

    /* a cheap error check: mktime returns "(time_t)-1L" on conversion errors.
     * Normally, we would have to apply a consistency check because "-1"
     * could also be a valid time. But, it is quite unlikely to read back odd
     * time numbers from file systems that store time stamps in DOS format.
     * (The only known exception is creation time on VFAT partitions.)
     */
    return (*ut != (time_t)-1L);
#endif /* ?HAVE_MKTIME */

} /* end function VFatFileTime2utime() */
#endif /* W32_STAT_BANDAID */



/******************************/
/* Function FStampIsLocTime() */
/******************************/

static int FStampIsLocTime(__GPRO__ const char *path)
{
    return (NTQueryVolInfo(__G__ path) ? G.lastVolLocTim : FALSE);
}



#ifndef NO_W32TIMES_IZFIX
# define UTIME_2_IZFILETIME(ut, pft) \
   if (fs_uses_loctime) {utime2VFatFileTime(ut, pft, TRUE);} \
   else {utime2NtfsFileTime(ut, pft);}
#else
# define UTIME_2_IZFILETIME(ut, pft) \
   utime2VFatFileTime(ut, pft, fs_uses_loctime);
#endif



/****************************/      /* Get the file time in a format that */
/* Function getNTfiletime() */      /*  can be used by SetFileTime() in NT */
/****************************/

static int getNTfiletime(__G__ pModFT, pAccFT, pCreFT)
    __GDEF
    FILETIME *pModFT;
    FILETIME *pAccFT;
    FILETIME *pCreFT;
{
#ifdef USE_EF_UT_TIME
    unsigned eb_izux_flg;
    iztimes z_utime;   /* struct for Unix-style actime & modtime, + creatime */
#endif
    int fs_uses_loctime = FStampIsLocTime(__G__ G.filename);

    /* Copy and/or convert time and date variables, if necessary;
     * return a flag indicating which time stamps are available. */
#ifdef USE_EF_UT_TIME
    if (G.extra_field &&
#ifdef IZ_CHECK_TZ
        G.tz_is_valid &&
#endif
        ((eb_izux_flg = ef_scan_for_izux(G.extra_field,
          G.lrec.extra_field_length, 0, G.lrec.last_mod_dos_datetime,
          &z_utime, NULL)) & EB_UT_FL_MTIME))
    {
        TTrace((stderr, "getNTfiletime:  Unix e.f. modif. time = %lu\n",
          z_utime.mtime));
        UTIME_2_IZFILETIME(z_utime.mtime, pModFT)
        if (eb_izux_flg & EB_UT_FL_ATIME) {
            UTIME_2_IZFILETIME(z_utime.atime, pAccFT)
        }
        if (eb_izux_flg & EB_UT_FL_CTIME) {
            UTIME_2_IZFILETIME(z_utime.ctime, pCreFT)
        }
        return (int)eb_izux_flg;
    }
#endif /* USE_EF_UT_TIME */
#ifndef NO_W32TIMES_IZFIX
    if (!fs_uses_loctime) {
        time_t ux_modtime;

        ux_modtime = dos_to_unix_time(G.lrec.last_mod_dos_datetime);
        utime2NtfsFileTime(ux_modtime, pModFT);
    } else
#endif /* NO_W32TIMES_IZFIX */
    {
        FILETIME lft;

        DosDateTimeToFileTime((WORD)(G.lrec.last_mod_dos_datetime >> 16),
                              (WORD)(G.lrec.last_mod_dos_datetime & 0xFFFFL),
                              &lft);
        LocalFileTimeToFileTime(&lft, pModFT);
    }
    *pAccFT = *pModFT;
    return (EB_UT_FL_MTIME | EB_UT_FL_ATIME);

} /* end function getNTfiletime() */




/**************************/
/* Function SetFileSize() */
/**************************/

int SetFileSize(FILE *file, zusz_t filesize)
{
#ifdef __RSXNT__
    /* RSXNT environment lacks a translation function from C file pointer
       to Win32-API file handle. So, simply do nothing. */
    return 0;
#else /* !__RSXNT__ */
    /* not yet verified, if that really creates an unfragmented file
      rommel@ars.de
     */
    HANDLE os_fh;
#ifdef Z_UINT8_DEFINED
    LARGE_INTEGER fsbuf;
#endif

    /* Win9x supports FAT file system, only; presetting file size does
       not help to prevent fragmentation. */
    if (!IsWinNT()) return 0;

    /* Win32-API calls require access to the Win32 file handle.
       The interface function used to retrieve the Win32 handle for
       a file opened by the C rtl is non-standard and may not be
       available for every Win32 compiler environment.
       (see also win32/win32.c of the Zip distribution)
     */
    os_fh = (HANDLE)_get_osfhandle(fileno(file));
    /* move file pointer behind the last byte of the expected file size */
#ifdef Z_UINT8_DEFINED
    fsbuf.QuadPart = filesize;
    if ((SetFilePointer(os_fh, fsbuf.LowPart, &fsbuf.HighPart, FILE_BEGIN)
         == 0xFFFFFFFF) && GetLastError() != NO_ERROR)
#else
    if (SetFilePointer(os_fh, (ulg)filesize, 0, FILE_BEGIN) == 0xFFFFFFFF)
#endif
        return -1;
    /* extend/truncate file to the current position */
    if (SetEndOfFile(os_fh) == 0)
        return -1;
    /* move file position pointer back to the start of the file! */
    return (SetFilePointer(os_fh, 0, 0, FILE_BEGIN) == 0xFFFFFFFF) ? -1 : 0;
#endif /* ?__RSXNT__ */
} /* end function SetFileSize() */




/****************************/
/* Function close_outfile() */
/****************************/

void close_outfile(__G)
    __GDEF
{
    FILETIME Modft;    /* File time type defined in NT, `last modified' time */
    FILETIME Accft;    /* NT file time type, `last access' time */
    FILETIME Creft;    /* NT file time type, `file creation' time */
    HANDLE hFile = INVALID_HANDLE_VALUE;        /* File handle defined in NT */
    int gotTime;
#ifdef NTSD_EAS
    uch *ebSDptr;
    unsigned ebSDlen;
#endif
#ifdef __RSXNT__        /* RSXNT/EMX C rtl uses OEM charset */
    char *ansi_name = (char *)alloca(strlen(G.filename) + 1);

    INTERN_TO_ISO(G.filename, ansi_name);
#   define Ansi_Fname  ansi_name
#else
#   define Ansi_Fname  G.filename
#endif

#ifndef __RSXNT__
    if (IsWinNT()) {
        /* Truncate the file to the current position.
         * This is needed to remove excess allocation in case the
         * extraction has failed or stopped prematurely. */
        SetEndOfFile((HANDLE)_get_osfhandle(fileno(G.outfile)));
    }
#endif

    /* Close the file and then re-open it using the Win32
     * CreateFile call, so that the file can be created
     * with GENERIC_WRITE access, otherwise the SetFileTime
     * call will fail. */
    fclose(G.outfile);

    /* don't set the time stamp and attributes on standard output */
    if (uO.cflag)
        return;

    /* skip restoring time stamps on user's request */
    if (uO.D_flag <= 1) {
        gotTime = getNTfiletime(__G__ &Modft, &Accft, &Creft);

        /* open a handle to the file before processing extra fields;
           we do this in case new security on file prevents us from updating
           time stamps */
        hFile = CreateFileA(Ansi_Fname, GENERIC_WRITE, FILE_SHARE_WRITE, NULL,
             OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, NULL);
    } else {
        gotTime = 0;
    }

    /* sfield@microsoft.com: set attributes before time in case we decide to
       support other filetime members later.  This also allows us to apply
       attributes before the security is changed, which may prevent this
       from succeeding otherwise.  Also, since most files don't have
       any interesting attributes, only change them if something other than
       FILE_ATTRIBUTE_ARCHIVE appears in the attributes.  This works well
       as an optimization because FILE_ATTRIBUTE_ARCHIVE gets applied to the
       file anyway, when it's created new. */
    if ((G.pInfo->file_attr & 0x7F) & ~FILE_ATTRIBUTE_ARCHIVE) {
        if (!SetFileAttributesA(Ansi_Fname, G.pInfo->file_attr & 0x7F))
            Info(slide, 1, ((char *)slide,
              "\nwarning (%d): could not set file attributes\n",
              (int)GetLastError()));
    }

#ifdef NTSD_EAS
    /* set NTFS SD extra fields */
    if (G.extra_field &&    /* zipfile extra field may have extended attribs */
        FindSDExtraField(__G__ G.extra_field, G.lrec.extra_field_length,
                         &ebSDptr, &ebSDlen))
    {
        int err = SetSD(__G__ Ansi_Fname, G.pInfo->file_attr,
                        ebSDptr, ebSDlen);

        if (err == IZ_EF_TRUNC) {
            if (uO.qflag)
                Info(slide, 1, ((char *)slide, "%-22s ",
                  FnFilter1(G.filename)));
            Info(slide, 1, ((char *)slide, LoadFarString(TruncNTSD),
              ebSDlen-(EB_NTSD_L_LEN+EB_CMPRHEADLEN), uO.qflag? "\n":""));
        }
    }
#endif /* NTSD_EAS */

    /* skip restoring time stamps on user's request */
    if (uO.D_flag <= 1) {
        if ( hFile == INVALID_HANDLE_VALUE )
            Info(slide, 1, ((char *)slide,
              "\nCreateFile() error %d when trying set file time\n",
              (int)GetLastError()));
        else {
            if (gotTime) {
                FILETIME *pModft = (gotTime & EB_UT_FL_MTIME) ? &Modft : NULL;
                FILETIME *pAccft = (gotTime & EB_UT_FL_ATIME) ? &Accft : NULL;
                FILETIME *pCreft = (gotTime & EB_UT_FL_CTIME) ? &Creft : NULL;

                if (!SetFileTime(hFile, pCreft, pAccft, pModft))
                    Info(slide, 0, ((char *)slide,
                      "\nSetFileTime failed: %d\n", (int)GetLastError()));
            }
            CloseHandle(hFile);
        }
    }

    return;

#undef Ansi_Fname

} /* end function close_outfile() */




#ifdef SET_DIR_ATTRIB

int defer_dir_attribs(__G__ pd)
    __GDEF
    direntry **pd;
{
    NTdirattr *d_entry;
#ifdef NTSD_EAS
    uch *ebSDptr;
    unsigned ebSDlen;
#endif

    /* Win9x does not support setting directory time stamps. */
    if (!IsWinNT()) {
        *pd = (direntry *)NULL;
        return PK_OK;
    }

#ifdef NTSD_EAS
    /* set extended attributes from extra fields */
    if (G.extra_field &&  /* zipfile e.f. may have extended attribs */
        FindSDExtraField(__G__ G.extra_field, G.lrec.extra_field_length,
                         &ebSDptr, &ebSDlen)) {
        /* ebSDlen contains the payload size of the e.f. block, but
           we store it including the e.b. header. */
        ebSDlen += EB_HEADSIZE;
    } else {
        /* no NTSD e.f. block -> no space needed to allocate */
        ebSDlen = 0;
    }
#endif /* NTSD_EAS */

    d_entry = (NTdirattr *)malloc(sizeof(NTdirattr)
#ifdef NTSD_EAS
                                  + ebSDlen
#endif
                                  + strlen(G.filename));
    *pd = (direntry *)d_entry;
    if (d_entry == (NTdirattr *)NULL) {
        return PK_MEM;
    }
#ifdef NTSD_EAS
    if (ebSDlen > 0)
        memcpy(d_entry->buf, ebSDptr, ebSDlen);
    d_entry->SDlen = ebSDlen;
    d_entry->fn = d_entry->buf + ebSDlen;
#else
    d_entry->fn = d_entry->buf;
#endif

    strcpy(d_entry->fn, G.filename);

    d_entry->perms = G.pInfo->file_attr;

    d_entry->gotTime = (uO.D_flag <= 0
                        ? getNTfiletime(__G__ &(d_entry->Modft),
                                        &(d_entry->Accft), &(d_entry->Creft))
                        : 0);
    return PK_OK;
} /* end function defer_dir_attribs() */


int set_direc_attribs(__G__ d)
    __GDEF
    direntry *d;
{
    int errval;
    HANDLE hFile = INVALID_HANDLE_VALUE;        /* File handle defined in NT */
#ifdef __RSXNT__
    char *ansi_name;
#endif

    /* Win9x does not support setting directory time stamps. */
    if (!IsWinNT())
        return PK_OK;

    errval = PK_OK;
#ifdef __RSXNT__        /* RSXNT/EMX C rtl uses OEM charset */
    ansi_name = (char *)alloca(strlen(d->fn) + 1);
    INTERN_TO_ISO(d->fn, ansi_name);
#   define Ansi_Dirname  ansi_name
#else
#   define Ansi_Dirname  d->fn
#endif

    /* Skip restoring directory time stamps on user' request. */
    if (uO.D_flag <= 0) {
        /* Open a handle to the directory before processing extra fields;
           we do this in case new security on file prevents us from updating
           time stamps.
           Although the WIN32 documentation recommends to use GENERIC_WRITE
           access flag to create the handle for SetFileTime(), this is too
           demanding for directories with the "read-only" attribute bit set.
           So we use the more specific flag FILE_WRITE_ATTRIBUTES here to
           request the minimum required access rights. (This problem is a
           Windows bug that has been silently fixed in Windows XP SP2.) */
        hFile = CreateFileA(Ansi_Dirname, FILE_WRITE_ATTRIBUTES,
                            FILE_SHARE_READ|FILE_SHARE_WRITE, NULL,
                            OPEN_EXISTING, FILE_FLAG_BACKUP_SEMANTICS, NULL);
    }

#ifdef NTSD_EAS
    if (NtAtt(d)->SDlen > 0) {
        int err;

        if (QCOND2) {
            Info(slide, 1, ((char *)slide, " set attrib: %-22s  ",
              FnFilter1(d->fn)));
        }

        /* set NTFS SD extra fields */
        err = SetSD(__G__ Ansi_Dirname, NtAtt(d)->perms,
                        NtAtt(d)->buf, NtAtt(d)->SDlen - EB_HEADSIZE);
        if (err == IZ_EF_TRUNC) {
            if (!QCOND2)
                Info(slide, 1, ((char *)slide, "%-22s  ",
                  FnFilter1(d->fn)));
            Info(slide, 1, ((char *)slide, LoadFarString(TruncNTSD),
              NtAtt(d)->SDlen-(EB_NTSD_L_LEN+EB_CMPRHEADLEN), "\n"));
        } else if (QCOND2) {
            Info(slide, 0, ((char *)slide, "\n"));
        }
        if (errval < err)
            errval = err;
    }
#endif /* NTSD_EAS */

    /* Skip restoring directory time stamps on user' request. */
    if (uO.D_flag <= 0) {
        if (hFile == INVALID_HANDLE_VALUE) {
            Info(slide, 1, ((char *)slide,
              "warning: CreateFile() error %d (set file times for %s)\n",
              (int)GetLastError(), FnFilter1(d->fn)));
            if (!errval)
                errval = PK_WARN;
        } else {
            if (NtAtt(d)->gotTime) {
                FILETIME *pModft = (NtAtt(d)->gotTime & EB_UT_FL_MTIME)
                                  ? &(NtAtt(d)->Modft) : NULL;
                FILETIME *pAccft = (NtAtt(d)->gotTime & EB_UT_FL_ATIME)
                                  ? &(NtAtt(d)->Accft) : NULL;
                FILETIME *pCreft = (NtAtt(d)->gotTime & EB_UT_FL_CTIME)
                                  ? &(NtAtt(d)->Creft) : NULL;

                if (!SetFileTime(hFile, pCreft, pAccft, pModft)) {
                    Info(slide, 0, ((char *)slide,
                      "warning:  SetFileTime() for %s error %d\n",
                      FnFilter1(d->fn), (int)GetLastError()));
                    if (!errval)
                        errval = PK_WARN;
                }
            }
            CloseHandle(hFile);
        }
    }

    return errval;
} /* end function set_direc_attribs() */

#endif /* SET_DIR_ATTRIB */




#ifdef TIMESTAMP

/*************************/
/* Function stamp_file() */
/*************************/

int stamp_file(__GPRO__ ZCONST char *fname, time_t modtime)
{
    FILETIME Modft;    /* File time type defined in NT, `last modified' time */
    HANDLE hFile;      /* File handle defined in NT    */
    int errstat = 0;   /* return status: 0 == "OK", -1 == "Failure" */
    int fs_uses_loctime = FStampIsLocTime(__G__ fname);
#ifdef __RSXNT__        /* RSXNT/EMX C rtl uses OEM charset */
    char *ansi_name = (char *)alloca(strlen(fname) + 1);

    INTERN_TO_ISO(fname, ansi_name);
#   define Ansi_Fname  ansi_name
#else
#   define Ansi_Fname  fname
#endif

    /* open a handle to the file to prepare setting the mod-time stamp */
    hFile = CreateFileA(Ansi_Fname, GENERIC_WRITE, FILE_SHARE_WRITE, NULL,
         OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, NULL);
    if ( hFile == INVALID_HANDLE_VALUE ) {
        errstat = -1;
    } else {
        /* convert time_t modtime into WIN32 native 64bit format */
        UTIME_2_IZFILETIME(modtime, &Modft)
        /* set Access and Modification times of the file to modtime */
        if (!SetFileTime(hFile, NULL, &Modft, &Modft)) {
            errstat = -1;
        }
        CloseHandle(hFile);
    }

    return errstat;

#undef Ansi_Fname
} /* end function stamp_file() */

#endif /* TIMESTAMP */




/***********************/
/* Function isfloppy() */   /* more precisely, is it removable? */
/***********************/

static int isfloppy(int nDrive)   /* 1 == A:, 2 == B:, etc. */
{
    char rootPathName[4];

    rootPathName[0] = (char)('A' + nDrive - 1);   /* build the root path */
    rootPathName[1] = ':';                        /*  name, e.g. "A:/" */
    rootPathName[2] = '/';
    rootPathName[3] = '\0';

    return (GetDriveTypeA(rootPathName) == DRIVE_REMOVABLE);

} /* end function isfloppy() */




/*****************************/
/* Function NTQueryVolInfo() */
/*****************************/

/*
 * Note:  8.3 limits on filenames apply only to old-style FAT filesystems.
 *        More recent versions of Windows (Windows NT 3.5 / Windows 4.0)
 *        can support long filenames (LFN) on FAT filesystems.  Check the
 *        filesystem maximum component length field to detect LFN support.
 */

static int NTQueryVolInfo(__GPRO__ const char *name)
{
 /* static char lastRootPath[4] = ""; */
 /* static int lastVolOldFAT; */
 /* static int lastVolLocTim; */
    char *tmp0;
    char tmp1[MAX_PATH], tmp2[MAX_PATH];
    DWORD volSerNo, maxCompLen, fileSysFlags;
#ifdef __RSXNT__        /* RSXNT/EMX C rtl uses OEM charset */
    char *ansi_name = (char *)alloca(strlen(name) + 1);

    INTERN_TO_ISO(name, ansi_name);
    name = ansi_name;
#endif

    if ((!strncmp(name, "//", 2) || !strncmp(name, "\\\\", 2)) &&
        (name[2] != '\0' && name[2] != '/' && name[2] != '\\')) {
        /* GetFullPathname() and GetVolumeInformation() do not work
         * on UNC names. For now, we return "error".
         * **FIXME**: check if UNC name is mapped to a drive letter
         *            and use mapped drive for volume info query.
         */
        return FALSE;
    }
    if (isalpha((uch)name[0]) && (name[1] == ':'))
        tmp0 = (char *)name;
    else
    {
        if (!GetFullPathNameA(name, MAX_PATH, tmp1, &tmp0))
            return FALSE;
        tmp0 = &tmp1[0];
    }
    if (strncmp(G.lastRootPath, tmp0, 2) != 0) {
        /* For speed, we skip repeated queries for the same device */
        strncpy(G.lastRootPath, tmp0, 2);   /* Build the root path name, */
        G.lastRootPath[2] = '/';            /* e.g. "A:/"                */
        G.lastRootPath[3] = '\0';

        if (!GetVolumeInformationA((LPCSTR)G.lastRootPath,
              (LPSTR)tmp1, (DWORD)MAX_PATH,
              &volSerNo, &maxCompLen, &fileSysFlags,
              (LPSTR)tmp2, (DWORD)MAX_PATH)) {
            G.lastRootPath[0] = '\0';
            return FALSE;
        }

        /*  LFNs are available if the component length is > 12 */
        G.lastVolOldFAT = (maxCompLen <= 12);
/*      G.lastVolOldFAT = !strncmp(strupr(tmp2), "FAT", 3);   old version */

        /* Volumes in (V)FAT and (OS/2) HPFS format store file timestamps in
         * local time!
         */
        G.lastVolLocTim = !strncmp(strupr(tmp2), "VFAT", 4) ||
                          !strncmp(tmp2, "HPFS", 4) ||
                          !strncmp(tmp2, "FAT", 3);
    }

    return TRUE;

} /* end function NTQueryVolInfo() */




/*****************************/
/* Function IsVolumeOldFAT() */
/*****************************/

static int IsVolumeOldFAT(__GPRO__ const char *name)
{
    return (NTQueryVolInfo(__G__ name) ? G.lastVolOldFAT : FALSE);
}




#ifndef SFX

/************************/
/*  Function do_wild()  */   /* identical to OS/2 version */
/************************/

char *do_wild(__G__ wildspec)
    __GDEF
    ZCONST char *wildspec;  /* only used first time on a given dir */
{
/* these statics are now declared in SYSTEM_SPECIFIC_GLOBALS in w32cfg.h:
    static zDIR *wild_dir = NULL;
    static ZCONST char *wildname;
    static char *dirname, matchname[FILNAMSIZ];
    static int notfirstcall=FALSE, have_dirname, dirnamelen;
*/
    char *fnamestart;
    struct zdirent *file;

    /* Even when we're just returning wildspec, we *always* do so in
     * matchname[]--calling routine is allowed to append four characters
     * to the returned string, and wildspec may be a pointer to argv[].
     */
    if (!G.notfirstcall) {  /* first call:  must initialize everything */
        G.notfirstcall = TRUE;

        if (!iswild(wildspec)) {
            strncpy(G.matchname, wildspec, FILNAMSIZ);
            G.matchname[FILNAMSIZ-1] = '\0';
            G.have_dirname = FALSE;
            G.wild_dir = NULL;
            return G.matchname;
        }

        /* break the wildspec into a directory part and a wildcard filename */
        if ((G.wildname = MBSRCHR(wildspec, '/')) == (ZCONST char *)NULL &&
            (G.wildname = MBSRCHR(wildspec, ':')) == (ZCONST char *)NULL) {
            G.dirname = ".";
            G.dirnamelen = 1;
            G.have_dirname = FALSE;
            G.wildname = wildspec;
        } else {
            ++G.wildname;     /* point at character after '/' or ':' */
            G.dirnamelen = G.wildname - wildspec;
            if ((G.dirname = (char *)malloc(G.dirnamelen+1)) == NULL) {
                Info(slide, 1, ((char *)slide,
                  "warning:  cannot allocate wildcard buffers\n"));
                strncpy(G.matchname, wildspec, FILNAMSIZ);
                G.matchname[FILNAMSIZ-1] = '\0';
                return G.matchname; /* but maybe filespec was not a wildcard */
            }
            strncpy(G.dirname, wildspec, G.dirnamelen);
            G.dirname[G.dirnamelen] = '\0';   /* terminate for strcpy below */
            G.have_dirname = TRUE;
        }
        Trace((stderr, "do_wild:  dirname = [%s]\n", FnFilter1(G.dirname)));

        if ((G.wild_dir = (zvoid *)Opendir(G.dirname)) != NULL) {
            if (G.have_dirname) {
                strcpy(G.matchname, G.dirname);
                fnamestart = G.matchname + G.dirnamelen;
            } else
                fnamestart = G.matchname;
            while ((file = Readdir((zDIR *)G.wild_dir)) != NULL) {
                Trace((stderr, "do_wild:  Readdir returns %s\n",
                  FnFilter1(file->d_name)));
                strcpy(fnamestart, file->d_name);
                if (MBSRCHR(fnamestart, '.') == (char *)NULL)
                    strcat(fnamestart, ".");
                if (match(fnamestart, G.wildname, TRUE WISEP) &&
                    /* skip "." and ".." directory entries */
                    strcmp(fnamestart, ".") && strcmp(fnamestart, "..")) {
                    Trace((stderr, "do_wild:  match() succeeds\n"));
                    /* remove trailing dot */
                    fnamestart = plastchar(fnamestart, strlen(fnamestart));
                    if (*fnamestart == '.')
                        *fnamestart = '\0';
                    return G.matchname;
                }
            }
            /* if we get to here directory is exhausted, so close it */
            Closedir((zDIR *)G.wild_dir);
            G.wild_dir = NULL;
        }
        Trace((stderr, "do_wild:  Opendir(%s) returns NULL\n",
          FnFilter1(G.dirname)));

        /* return the raw wildspec in case that works (e.g., directory not
         * searchable, but filespec was not wild and file is readable) */
        strncpy(G.matchname, wildspec, FILNAMSIZ);
        G.matchname[FILNAMSIZ-1] = '\0';
        return G.matchname;
    }

    /* last time through, might have failed opendir but returned raw wildspec */
    if (G.wild_dir == NULL) {
        G.notfirstcall = FALSE;    /* reset for new wildspec */
        if (G.have_dirname)
            free(G.dirname);
        return (char *)NULL;
    }

    /* If we've gotten this far, we've read and matched at least one entry
     * successfully (in a previous call), so dirname has been copied into
     * matchname already.
     */
    if (G.have_dirname) {
        /* strcpy(G.matchname, G.dirname); */
        fnamestart = G.matchname + G.dirnamelen;
    } else
        fnamestart = G.matchname;
    while ((file = Readdir((zDIR *)G.wild_dir)) != NULL) {
        Trace((stderr, "do_wild:  readdir returns %s\n",
          FnFilter1(file->d_name)));
        strcpy(fnamestart, file->d_name);
        if (MBSRCHR(fnamestart, '.') == (char *)NULL)
            strcat(fnamestart, ".");
        if (match(fnamestart, G.wildname, TRUE WISEP)) {
            Trace((stderr, "do_wild:  match() succeeds\n"));
            /* remove trailing dot */
            fnamestart = plastchar(fnamestart, strlen(fnamestart));
            if (*fnamestart == '.')
                *fnamestart = '\0';
            return G.matchname;
        }
    }

    Closedir((zDIR *)G.wild_dir);  /* at least one entry read; nothing left */
    G.wild_dir = NULL;
    G.notfirstcall = FALSE;        /* reset for new wildspec */
    if (G.have_dirname)
        free(G.dirname);
    return (char *)NULL;

} /* end function do_wild() */

#endif /* !SFX */



/**********************/
/* Function mapattr() */
/**********************/

/* Identical to MS-DOS, OS/2 versions.  However, NT has a lot of extra
 * permission stuff, so this function should probably be extended in the
 * future. */

int mapattr(__G)
    __GDEF
{
    /* set archive bit for file entries (file is not backed up): */
    G.pInfo->file_attr = ((unsigned)G.crec.external_file_attributes |
      (G.crec.external_file_attributes & FILE_ATTRIBUTE_DIRECTORY ?
       0 : FILE_ATTRIBUTE_ARCHIVE)) & 0xff;
    return 0;

} /* end function mapattr() */




/************************/
/*  Function mapname()  */
/************************/

int mapname(__G__ renamed)
    __GDEF
    int renamed;
/*
 * returns:
 *  MPN_OK          - no problem detected
 *  MPN_INF_TRUNC   - caution (truncated filename)
 *  MPN_INF_SKIP    - info "skip entry" (dir doesn't exist)
 *  MPN_ERR_SKIP    - error -> skip entry
 *  MPN_ERR_TOOLONG - error -> path is too long
 *  MPN_NOMEM       - error (memory allocation failed) -> skip entry
 *  [also MPN_VOL_LABEL, MPN_CREATED_DIR]
 */
{
    char pathcomp[FILNAMSIZ];   /* path-component buffer */
    char *pp, *cp=NULL;         /* character pointers */
    char *lastsemi = NULL;      /* pointer to last semi-colon in pathcomp */
#ifdef ACORN_FTYPE_NFS
    char *lastcomma=(char *)NULL;  /* pointer to last comma in pathcomp */
    RO_extra_block *ef_spark;      /* pointer Acorn FTYPE ef block */
#endif
    int killed_ddot = FALSE;    /* is set when skipping "../" pathcomp */
    int error;
    register unsigned workch;   /* hold the character being tested */


/*---------------------------------------------------------------------------
    Initialize various pointers and counters and stuff.
  ---------------------------------------------------------------------------*/

    /* can create path as long as not just freshening, or if user told us */
    G.create_dirs = (!uO.fflag || renamed);

    G.created_dir = FALSE;      /* not yet */
    G.renamed_fullpath = FALSE;
    G.fnlen = strlen(G.filename);

    if (renamed) {
        cp = G.filename;    /* point to beginning of renamed name... */
        if (*cp) do {
            if (*cp == '\\')    /* convert backslashes to forward */
                *cp = '/';
        } while (*PREINCSTR(cp));
        cp = G.filename;
        /* use temporary rootpath if user gave full pathname */
        if (G.filename[0] == '/') {
            G.renamed_fullpath = TRUE;
            pathcomp[0] = '/';  /* copy the '/' and terminate */
            pathcomp[1] = '\0';
            ++cp;
        } else if (isalpha((uch)G.filename[0]) && G.filename[1] == ':') {
            G.renamed_fullpath = TRUE;
            pp = pathcomp;
            *pp++ = *cp++;      /* copy the "d:" (+ '/', possibly) */
            *pp++ = *cp++;
            if (*cp == '/')
                *pp++ = *cp++;  /* otherwise add "./"? */
            *pp = '\0';
        }
    }

    /* pathcomp is ignored unless renamed_fullpath is TRUE: */
    if ((error = checkdir(__G__ pathcomp, INIT)) != 0)    /* init path buffer */
        return error;           /* ...unless no mem or vol label on hard disk */

    *pathcomp = '\0';           /* initialize translation buffer */
    pp = pathcomp;              /* point to translation buffer */
    if (!renamed) {             /* cp already set if renamed */
        if (uO.jflag)           /* junking directories */
            cp = (char *)MBSRCHR(G.filename, '/');
        if (cp == NULL)         /* no '/' or not junking dirs */
            cp = G.filename;    /* point to internal zipfile-member pathname */
        else
            ++cp;               /* point to start of last component of path */
    }

/*---------------------------------------------------------------------------
    Begin main loop through characters in filename.
  ---------------------------------------------------------------------------*/

    for (; (workch = (uch)*cp) != 0; INCSTR(cp)) {

        switch (workch) {
            case '/':             /* can assume -j flag not given */
                *pp = '\0';
                maskDOSdevice(__G__ pathcomp);
                if (strcmp(pathcomp, ".") == 0) {
                    /* don't bother appending "./" to the path */
                    *pathcomp = '\0';
                } else if (!uO.ddotflag && strcmp(pathcomp, "..") == 0) {
                    /* "../" dir traversal detected, skip over it */
                    *pathcomp = '\0';
                    killed_ddot = TRUE;     /* set "show message" flag */
                }
                /* when path component is not empty, append it now */
                if (*pathcomp != '\0' &&
                    ((error = checkdir(__G__ pathcomp, APPEND_DIR))
                     & MPN_MASK) > MPN_INF_TRUNC)
                    return error;
                pp = pathcomp;    /* reset conversion buffer for next piece */
                lastsemi = (char *)NULL; /* leave direct. semi-colons alone */
                break;

            case ':':             /* drive spec not stored, so no colon allowed */
            case '\\':            /* '\\' may come as normal filename char (not */
            case '<':             /*  dir sep char!) from unix-like file system */
            case '>':             /* no redirection symbols allowed either */
            case '|':             /* no pipe signs allowed */
            case '"':             /* no double quotes allowed */
            case '?':             /* no wildcards allowed */
            case '*':
                *pp++ = '_';      /* these rules apply equally to FAT and NTFS */
                break;
            case ';':             /* start of VMS version? */
                lastsemi = pp;    /* remove VMS version later... */
                *pp++ = ';';      /*  but keep semicolon for now */
                break;

#ifdef ACORN_FTYPE_NFS
            case ',':             /* NFS filetype extension */
                lastcomma = pp;
                *pp++ = ',';      /* keep for now; may need to remove */
                break;            /*  later, if requested */
#endif

            case ' ':             /* keep spaces unless specifically */
                /* NT cannot create filenames with spaces on FAT volumes */
                if (uO.sflag || IsVolumeOldFAT(__G__ G.filename))
                    *pp++ = '_';
                else
                    *pp++ = ' ';
                break;

            default:
                /* allow European characters in filenames: */
                if (isprint(workch) || workch >= 127)
#ifdef _MBCS
                {
                    memcpy(pp, cp, CLEN(cp));
                    INCSTR(pp);
                }
#else
                    *pp++ = (char)workch;
#endif
        } /* end switch */

    } /* end while loop */

    /* Show warning when stripping insecure "parent dir" path components */
    if (killed_ddot && QCOND2) {
        Info(slide, 0, ((char *)slide,
          "warning:  skipped \"../\" path component(s) in %s\n",
          FnFilter1(G.filename)));
        if (!(error & ~MPN_MASK))
            error = (error & MPN_MASK) | PK_WARN;
    }

/*---------------------------------------------------------------------------
    Report if directory was created (and no file to create:  filename ended
    in '/'), check name to be sure it exists, and combine path and name be-
    fore exiting.
  ---------------------------------------------------------------------------*/

    if (lastchar(G.filename, G.fnlen) == '/') {
#ifdef __RSXNT__        /* RSXNT/EMX C rtl uses OEM charset */
        char *ansi_name = (char *)alloca(strlen(G.filename) + 1);

        INTERN_TO_ISO(G.filename, ansi_name);
#       define Ansi_Fname  ansi_name
#else
#       define Ansi_Fname  G.filename
#endif
        checkdir(__G__ G.filename, GETPATH);
        if (G.created_dir) {
            if (QCOND2) {
                Info(slide, 0, ((char *)slide, "   creating: %-22s\n",
                  FnFilter1(G.filename)));
            }

            /* set file attributes:
               The default for newly created directories is "DIR attribute
               flags set", so there is no need to change attributes unless
               one of the DOS style attribute flags is set. The readonly
               attribute need not be masked, since it does not prevent
               modifications in the new directory. */
            if(G.pInfo->file_attr & (0x7F & ~FILE_ATTRIBUTE_DIRECTORY)) {
                if (!SetFileAttributesA(Ansi_Fname, G.pInfo->file_attr & 0x7F))
                    Info(slide, 1, ((char *)slide,
                      "\nwarning (%d): could not set file attributes for %s\n",
                      (int)GetLastError(), FnFilter1(G.filename)));
            }

            /* set dir time (note trailing '/') */
            return (error & ~MPN_MASK) | MPN_CREATED_DIR;
        } else if (IS_OVERWRT_ALL) {
            /* overwrite attributes of existing directory on user's request */

            /* set file attributes: */
            if(G.pInfo->file_attr & (0x7F & ~FILE_ATTRIBUTE_DIRECTORY)) {
                if (!SetFileAttributesA(Ansi_Fname, G.pInfo->file_attr & 0x7F))
                    Info(slide, 1, ((char *)slide,
                      "\nwarning (%d): could not set file attributes for %s\n",
                      (int)GetLastError(), FnFilter1(G.filename)));
            }
        }
        /* dir existed already; don't look for data to extract */
        return (error & ~MPN_MASK) | MPN_INF_SKIP;
    }

    *pp = '\0';                   /* done with pathcomp:  terminate it */

    /* if not saving them, remove VMS version numbers (appended "###") */
    if (!uO.V_flag && lastsemi) {
        pp = lastsemi + 1;        /* semi-colon was kept:  expect #'s after */
        while (isdigit((uch)(*pp)))
            ++pp;
        if (*pp == '\0')          /* only digits between ';' and end:  nuke */
            *lastsemi = '\0';
    }

#ifdef ACORN_FTYPE_NFS
    /* translate Acorn filetype information if asked to do so */
    if (uO.acorn_nfs_ext &&
        (ef_spark = (RO_extra_block *)
                    getRISCOSexfield(G.extra_field, G.lrec.extra_field_length))
        != (RO_extra_block *)NULL)
    {
        /* file *must* have a RISC OS extra field */
        long ft = (long)makelong(ef_spark->loadaddr);
        /*32-bit*/
        if (lastcomma) {
            pp = lastcomma + 1;
            while (isxdigit((uch)(*pp))) ++pp;
            if (pp == lastcomma+4 && *pp == '\0') *lastcomma='\0'; /* nuke */
        }
        if ((ft & 1<<31)==0) ft=0x000FFD00;
        sprintf(pathcomp+strlen(pathcomp), ",%03x", (int)(ft>>8) & 0xFFF);
    }
#endif /* ACORN_FTYPE_NFS */

    maskDOSdevice(__G__ pathcomp);

    if (*pathcomp == '\0') {
        Info(slide, 1, ((char *)slide, "mapname:  conversion of %s failed\n",
          FnFilter1(G.filename)));
        return (error & ~MPN_MASK) | MPN_ERR_SKIP;
    }

    checkdir(__G__ pathcomp, APPEND_NAME);  /* returns 1 if truncated: care? */
    checkdir(__G__ G.filename, GETPATH);

    if (G.pInfo->vollabel) {    /* set the volume label now */
        char drive[4];
#ifdef __RSXNT__        /* RSXNT/EMX C rtl uses OEM charset */
        char *ansi_name = (char *)alloca(strlen(G.filename) + 1);
        INTERN_TO_ISO(G.filename, ansi_name);
#       define Ansi_Fname  ansi_name
#else
#       define Ansi_Fname  G.filename
#endif

        /* Build a drive string, e.g. "b:" */
        drive[0] = (char)('a' + G.nLabelDrive - 1);
        strcpy(drive + 1, ":\\");
        if (QCOND2)
            Info(slide, 0, ((char *)slide, "labelling %s %-22s\n", drive,
              FnFilter1(G.filename)));
        if (!SetVolumeLabelA(drive, Ansi_Fname)) {
            Info(slide, 1, ((char *)slide,
              "mapname:  error setting volume label\n"));
            return (error & ~MPN_MASK) | MPN_ERR_SKIP;
        }
        /* success:  skip the "extraction" quietly */
        return (error & ~MPN_MASK) | MPN_INF_SKIP;
#undef Ansi_Fname
    }

    Trace((stderr, "mapname returns with filename = [%s] (error = %d)\n\n",
      FnFilter1(G.filename), error));
    return error;

} /* end function mapname() */




/****************************/
/* Function maskDOSdevice() */
/****************************/

static void maskDOSdevice(__G__ pathcomp)
    __GDEF
    char *pathcomp;
{
/*---------------------------------------------------------------------------
    Put an underscore in front of the file name if the file name is a
    DOS/WINDOWS device name like CON.*, AUX.*, PRN.*, etc. Trying to
    extract such a file would fail at best and wedge us at worst.
  ---------------------------------------------------------------------------*/
#if !defined(S_IFCHR) && defined(_S_IFCHR)
#  define S_IFCHR _S_IFCHR
#endif
#if !defined(S_ISCHR)
# if defined(_S_ISCHR)
#  define S_ISCHR(m) _S_ISCHR(m)
# elif defined(S_IFCHR)
#  define S_ISCHR(m) ((m) & S_IFCHR)
# endif
#endif

#ifdef DEBUG
    if (zstat(pathcomp, &G.statbuf) == 0) {
        Trace((stderr,
               "maskDOSdevice() stat(\"%s\", buf) st_mode result: %X, %o\n",
               FnFilter1(pathcomp), G.statbuf.st_mode, G.statbuf.st_mode));
    } else {
        Trace((stderr, "maskDOSdevice() stat(\"%s\", buf) failed\n",
               FnFilter1(pathcomp)));
    }
#endif
    if (zstat(pathcomp, &G.statbuf) == 0 && S_ISCHR(G.statbuf.st_mode)) {
        extent i;

        /* pathcomp contains a name of a DOS character device (builtin or
         * installed device driver).
         * Prepend a '_' to allow creation of the item in the file system.
         */
        for (i = strlen(pathcomp) + 1; i > 0; --i)
            pathcomp[i] = pathcomp[i - 1];
        pathcomp[0] = '_';
    }
} /* end function maskDOSdevice() */





/**********************/
/* Function map2fat() */        /* Not quite identical to OS/2 version */
/**********************/

static void map2fat(pathcomp, pEndFAT)
    char *pathcomp, **pEndFAT;
{
    char *ppc = pathcomp;       /* variable pointer to pathcomp */
    char *pEnd = *pEndFAT;      /* variable pointer to buildpathFAT */
    char *pBegin = *pEndFAT;    /* constant pointer to start of this comp. */
    char *last_dot = NULL;      /* last dot not converted to underscore */
    register unsigned workch;   /* hold the character being tested */


    /* Only need check those characters which are legal in NTFS but not
     * in FAT:  to get here, must already have passed through mapname.
     * Also must truncate path component to ensure 8.3 compliance.
     */
    while ((workch = (uch)*ppc++) != 0) {
        switch (workch) {
            case '[':
            case ']':
            case '+':
            case ',':
            case ';':
            case '=':
                *pEnd++ = '_';      /* convert brackets to underscores */
                break;

            case '.':
                if (pEnd == *pEndFAT) {   /* nothing appended yet... */
                    if (*ppc == '\0')     /* don't bother appending a */
                        break;            /*  "./" component to the path */
                    else if (*ppc == '.' && ppc[1] == '\0') {   /* "../" */
                        *pEnd++ = '.';    /*  add first dot, */
                        *pEnd++ = '.';    /*  add second dot, and */
                        ++ppc;            /*  skip over to pathcomp's end */
                    } else {              /* FAT doesn't allow null filename */
                        *pEnd++ = '_';    /*  bodies, so map .exrc -> _exrc */
                    }                     /*  (_.exr would keep max 3 chars) */
                } else {                  /* found dot within path component */
                    last_dot = pEnd;      /*  point at last dot so far... */
                    *pEnd++ = '_';        /*  convert to underscore for now */
                }
                break;

            default:
                *pEnd++ = (char)workch;

        } /* end switch */
    } /* end while loop */

    *pEnd = '\0';                 /* terminate buildpathFAT */

    /* NOTE:  keep in mind that pEnd points to the end of the path
     * component, and *pEndFAT still points to the *beginning* of it...
     * Also note that the algorithm does not try to get too fancy:
     * if there are no dots already, the name either gets truncated
     * at 8 characters or the last underscore is converted to a dot
     * (only if more characters are saved that way).  In no case is
     * a dot inserted between existing characters.
     */
    if (last_dot == NULL) {       /* no dots:  check for underscores... */
        char *plu = MBSRCHR(pBegin, '_');   /* pointer to last underscore */

        if ((plu != NULL) &&      /* found underscore: convert to dot? */
            (MIN(plu - pBegin, 8) + MIN(pEnd - plu - 1, 3) > 8)) {
            last_dot = plu;       /* be lazy:  drop through to next if-blk */
        } else if ((pEnd - *pEndFAT) > 8) {
            /* no underscore; or converting underscore to dot would save less
               chars than leaving everything in the basename */
            *pEndFAT += 8;        /* truncate at 8 chars */
            **pEndFAT = '\0';
        } else
            *pEndFAT = pEnd;      /* whole thing fits into 8 chars or less */
    }

    if (last_dot != NULL) {       /* one dot is OK: */
        *last_dot = '.';          /* put it back in */

        if ((last_dot - pBegin) > 8) {
            char *p, *q;
            int i;

            p = last_dot;
            q = last_dot = pBegin + 8;
            for (i = 0;  (i < 4) && *p;  ++i)  /* too many chars in basename: */
                *q++ = *p++;                   /*  shift .ext left and trun- */
            *q = '\0';                         /*  cate/terminate it */
            *pEndFAT = q;
        } else if ((pEnd - last_dot) > 4) {    /* too many chars in extension */
            *pEndFAT = last_dot + 4;
            **pEndFAT = '\0';
        } else
            *pEndFAT = pEnd;   /* filename is fine; point at terminating zero */

        if ((last_dot - pBegin) > 0 && last_dot[-1] == ' ')
            last_dot[-1] = '_';                /* NO blank in front of '.'! */
    }
} /* end function map2fat() */




/***********************/       /* Borrowed from os2.c for UnZip 5.1.        */
/* Function checkdir() */       /* Difference: no EA stuff                   */
/***********************/       /*             HPFS stuff works on NTFS too  */

int checkdir(__G__ pathcomp, flag)
    __GDEF
    char *pathcomp;
    int flag;
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
{
 /* static int rootlen = 0;     */   /* length of rootpath */
 /* static char *rootpath;      */   /* user's "extract-to" directory */
 /* static char *buildpathHPFS; */   /* full path (so far) to extracted file, */
 /* static char *buildpathFAT;  */   /*  both HPFS/EA (main) and FAT versions */
 /* static char *endHPFS;       */   /* corresponding pointers to end of */
 /* static char *endFAT;        */   /*  buildpath ('\0') */

#   define FN_MASK   7
#   define FUNCTION  (flag & FN_MASK)



/*---------------------------------------------------------------------------
    APPEND_DIR:  append the path component to the path being built and check
    for its existence.  If doesn't exist and we are creating directories, do
    so for this one; else signal success or error as appropriate.
  ---------------------------------------------------------------------------*/

    if (FUNCTION == APPEND_DIR) {
        char *p = pathcomp;
        int too_long = FALSE;

        Trace((stderr, "appending dir segment [%s]\n", FnFilter1(pathcomp)));
        while ((*G.endHPFS = *p++) != '\0')     /* copy to HPFS filename */
            ++G.endHPFS;
        if (!IsVolumeOldFAT(__G__ G.buildpathHPFS)) {
            p = pathcomp;
            while ((*G.endFAT = *p++) != '\0')  /* copy to FAT filename, too */
                ++G.endFAT;
        } else
            map2fat(pathcomp, &G.endFAT);   /* map into FAT fn, update endFAT */

        /* GRR:  could do better check, see if overrunning buffer as we go:
         * check endHPFS-buildpathHPFS after each append, set warning variable
         * if within 20 of FILNAMSIZ; then if var set, do careful check when
         * appending.  Clear variable when begin new path. */

        /* next check:  need to append '/', at least one-char name, '\0' */
        if ((G.endHPFS-G.buildpathHPFS) > FILNAMSIZ-3)
            too_long = TRUE;                    /* check if extracting dir? */
#ifdef FIX_STAT_BUG
        /* Borland C++ 5.0 does not handle a call to stat() well if the
         * directory does not exist (it tends to crash in strange places.)
         * This is apparently a problem only when compiling for GUI rather
         * than console. The code below attempts to work around this problem.
         */
        if (access(G.buildpathFAT, 0) != 0) {
            if (!G.create_dirs) { /* told not to create (freshening) */
                free(G.buildpathHPFS);
                free(G.buildpathFAT);
                /* path doesn't exist:  nothing to do */
                return MPN_INF_SKIP;
            }
            if (too_long) {   /* GRR:  should allow FAT extraction w/o EAs */
                Info(slide, 1, ((char *)slide,
                  "checkdir error:  path too long: %s\n",
                  FnFilter1(G.buildpathHPFS)));
                free(G.buildpathHPFS);
                free(G.buildpathFAT);
                /* no room for filenames:  fatal */
                return MPN_ERR_TOOLONG;
            }
            if (MKDIR(G.buildpathFAT, 0777) == -1) { /* create the directory */
                Info(slide, 1, ((char *)slide,
                  "checkdir error:  cannot create %s\n\
                 %s\n\
                 unable to process %s.\n",
                  FnFilter2(G.buildpathFAT),
                  strerror(errno),
                  FnFilter1(G.filename)));
                free(G.buildpathHPFS);
                free(G.buildpathFAT);
                /* path didn't exist, tried to create, failed */
                return MPN_ERR_SKIP;
            }
            G.created_dir = TRUE;
        }
#endif /* FIX_STAT_BUG */
        if (SSTAT(G.buildpathFAT, &G.statbuf))   /* path doesn't exist */
        {
            if (!G.create_dirs) { /* told not to create (freshening) */
                free(G.buildpathHPFS);
                free(G.buildpathFAT);
                /* path doesn't exist:  nothing to do */
                return MPN_INF_SKIP;
            }
            if (too_long) {   /* GRR:  should allow FAT extraction w/o EAs */
                Info(slide, 1, ((char *)slide,
                  "checkdir error:  path too long: %s\n",
                  FnFilter1(G.buildpathHPFS)));
                free(G.buildpathHPFS);
                free(G.buildpathFAT);
                /* no room for filenames:  fatal */
                return MPN_ERR_TOOLONG;
            }
            if (MKDIR(G.buildpathFAT, 0777) == -1) { /* create the directory */
                Info(slide, 1, ((char *)slide,
                  "checkdir error:  cannot create %s\n\
                 %s\n\
                 unable to process %s.\n",
                  FnFilter2(G.buildpathFAT),
                  strerror(errno),
                  FnFilter1(G.filename)));
                free(G.buildpathHPFS);
                free(G.buildpathFAT);
                /* path didn't exist, tried to create, failed */
                return MPN_ERR_SKIP;
            }
            G.created_dir = TRUE;
        } else if (!S_ISDIR(G.statbuf.st_mode)) {
            Info(slide, 1, ((char *)slide,
              "checkdir error:  %s exists but is not directory\n\
                 unable to process %s.\n",
              FnFilter2(G.buildpathFAT), FnFilter1(G.filename)));
            free(G.buildpathHPFS);
            free(G.buildpathFAT);
            /* path existed but wasn't dir */
            return MPN_ERR_SKIP;
        }
        if (too_long) {
            Info(slide, 1, ((char *)slide,
              "checkdir error:  path too long: %s\n",
              FnFilter1(G.buildpathHPFS)));
            free(G.buildpathHPFS);
            free(G.buildpathFAT);
            /* no room for filenames:  fatal */
            return MPN_ERR_TOOLONG;
        }
        *G.endHPFS++ = '/';
        *G.endFAT++ = '/';
        *G.endHPFS = *G.endFAT = '\0';
        Trace((stderr, "buildpathHPFS now = [%s]\nbuildpathFAT now =  [%s]\n",
          FnFilter1(G.buildpathHPFS), FnFilter2(G.buildpathFAT)));
        return MPN_OK;

    } /* end if (FUNCTION == APPEND_DIR) */

/*---------------------------------------------------------------------------
    GETPATH:  copy full FAT path to the string pointed at by pathcomp (want
    filename to reflect name used on disk, not EAs; if full path is HPFS,
    buildpathFAT and buildpathHPFS will be identical).  Also free both paths.
  ---------------------------------------------------------------------------*/

    if (FUNCTION == GETPATH) {
        Trace((stderr, "getting and freeing FAT path [%s]\n",
          FnFilter1(G.buildpathFAT)));
        Trace((stderr, "freeing HPFS path [%s]\n",
          FnFilter1(G.buildpathHPFS)));
        strcpy(pathcomp, G.buildpathFAT);
        free(G.buildpathFAT);
        free(G.buildpathHPFS);
        G.buildpathHPFS = G.buildpathFAT = G.endHPFS = G.endFAT = NULL;
        return MPN_OK;
    }

/*---------------------------------------------------------------------------
    APPEND_NAME:  assume the path component is the filename; append it and
    return without checking for existence.
  ---------------------------------------------------------------------------*/

    if (FUNCTION == APPEND_NAME) {
        char *p = pathcomp;
        int error = MPN_OK;

        Trace((stderr, "appending filename [%s]\n", FnFilter1(pathcomp)));
        /* The buildpathHPFS buffer has been allocated large enough to
         * hold the complete combined name, so there is no need to check
         * for OS filename size limit overflow within the copy loop.
         */
        while ((*G.endHPFS = *p++) != '\0') {   /* copy to HPFS filename */
            ++G.endHPFS;
        }
        /* Now, check for OS filename size overflow.  When detected, the
         * mapped HPFS name is truncated and a warning message is shown.
         */
        if ((G.endHPFS-G.buildpathHPFS) >= FILNAMSIZ) {
            G.buildpathHPFS[FILNAMSIZ-1] = '\0';
            Info(slide, 1, ((char *)slide,
              "checkdir warning:  path too long; truncating\n \
              %s\n                -> %s\n",
              FnFilter1(G.filename), FnFilter2(G.buildpathHPFS)));
            error = MPN_INF_TRUNC;  /* filename truncated */
        }

        /* The buildpathFAT buffer has the same allocated size as the
         * buildpathHPFS buffer, so there is no need for an overflow check
         * within the following copy loop, either.
         */
        if (G.pInfo->vollabel || !IsVolumeOldFAT(__G__ G.buildpathHPFS)) {
            /* copy to FAT filename, too */
            p = pathcomp;
            while ((*G.endFAT = *p++) != '\0')
                ++G.endFAT;
        } else
            /* map into FAT fn, update endFAT */
            map2fat(pathcomp, &G.endFAT);

        /* Check that the FAT path does not exceed the FILNAMSIZ limit, and
         * truncate when neccessary.
         * Note that truncation can only happen when the HPFS path (which is
         * never shorter than the FAT path) has been already truncated.
         * So, emission of the warning message and setting the error code
         * has already happened.
         */
        if ((G.endFAT-G.buildpathFAT) >= FILNAMSIZ)
            G.buildpathFAT[FILNAMSIZ-1] = '\0';
        Trace((stderr, "buildpathHPFS: %s\nbuildpathFAT:  %s\n",
          FnFilter1(G.buildpathHPFS), FnFilter2(G.buildpathFAT)));

        return error;  /* could check for existence, prompt for new name... */

    } /* end if (FUNCTION == APPEND_NAME) */

/*---------------------------------------------------------------------------
    INIT:  allocate and initialize buffer space for the file currently being
    extracted.  If file was renamed with an absolute path, don't prepend the
    extract-to path.
  ---------------------------------------------------------------------------*/

    if (FUNCTION == INIT) {
        Trace((stderr, "initializing buildpathHPFS and buildpathFAT to "));
#ifdef ACORN_FTYPE_NFS
        if ((G.buildpathHPFS = (char *)malloc(G.fnlen+G.rootlen+
                                              (uO.acorn_nfs_ext ? 5 : 1)))
#else
        if ((G.buildpathHPFS = (char *)malloc(G.fnlen+G.rootlen+1))
#endif
            == NULL)
            return MPN_NOMEM;
#ifdef ACORN_FTYPE_NFS
        if ((G.buildpathFAT = (char *)malloc(G.fnlen+G.rootlen+
                                             (uO.acorn_nfs_ext ? 5 : 1)))
#else
        if ((G.buildpathFAT = (char *)malloc(G.fnlen+G.rootlen+1))
#endif
            == NULL) {
            free(G.buildpathHPFS);
            return MPN_NOMEM;
        }
        if (G.pInfo->vollabel) { /* use root or renamed path, but don't store */
/* GRR:  for network drives, do strchr() and return IZ_VOL_LABEL if not [1] */
            if (G.renamed_fullpath && pathcomp[1] == ':')
                *G.buildpathHPFS = (char)ToLower(*pathcomp);
            else if (!G.renamed_fullpath && G.rootlen > 1 &&
                     G.rootpath[1] == ':')
                *G.buildpathHPFS = (char)ToLower(*G.rootpath);
            else {
                char tmpN[MAX_PATH], *tmpP;
                if (GetFullPathNameA(".", MAX_PATH, tmpN, &tmpP) > MAX_PATH)
                { /* by definition of MAX_PATH we should never get here */
                    Info(slide, 1, ((char *)slide,
                      "checkdir warning: current dir path too long\n"));
                    return MPN_INF_TRUNC;   /* can't get drive letter */
                }
                G.nLabelDrive = *tmpN - 'a' + 1;
                *G.buildpathHPFS = (char)(G.nLabelDrive - 1 + 'a');
            }
            G.nLabelDrive = *G.buildpathHPFS - 'a' + 1; /* save for mapname() */
            if (uO.volflag == 0 || *G.buildpathHPFS < 'a' /* no labels/bogus? */
                || (uO.volflag == 1 && !isfloppy(G.nLabelDrive))) { /* !fixed */
                free(G.buildpathHPFS);
                free(G.buildpathFAT);
                return MPN_VOL_LABEL;  /* skipping with message */
            }
            *G.buildpathHPFS = '\0';
        } else if (G.renamed_fullpath) /* pathcomp = valid data */
            strcpy(G.buildpathHPFS, pathcomp);
        else if (G.rootlen > 0)
            strcpy(G.buildpathHPFS, G.rootpath);
        else
            *G.buildpathHPFS = '\0';
        G.endHPFS = G.buildpathHPFS;
        G.endFAT = G.buildpathFAT;
        while ((*G.endFAT = *G.endHPFS) != '\0') {
            ++G.endFAT;
            ++G.endHPFS;
        }
        Trace((stderr, "[%s]\n", FnFilter1(G.buildpathHPFS)));
        return MPN_OK;
    }

/*---------------------------------------------------------------------------
    ROOT:  if appropriate, store the path in rootpath and create it if neces-
    sary; else assume it's a zipfile member and return.  This path segment
    gets used in extracting all members from every zipfile specified on the
    command line.  Note that under OS/2 and MS-DOS, if a candidate extract-to
    directory specification includes a drive letter (leading "x:"), it is
    treated just as if it had a trailing '/'--that is, one directory level
    will be created if the path doesn't exist, unless this is otherwise pro-
    hibited (e.g., freshening).
  ---------------------------------------------------------------------------*/

#if (!defined(SFX) || defined(SFX_EXDIR))
    if (FUNCTION == ROOT) {
        Trace((stderr, "initializing root path to [%s]\n",
          FnFilter1(pathcomp)));
        if (pathcomp == NULL) {
            G.rootlen = 0;
            return MPN_OK;
        }
        if (G.rootlen > 0)      /* rootpath was already set, nothing to do */
            return MPN_OK;
        if ((G.rootlen = strlen(pathcomp)) > 0) {
            int had_trailing_pathsep=FALSE, has_drive=FALSE, add_dot=FALSE;
            char *tmproot;

            if ((tmproot = (char *)malloc(G.rootlen+3)) == (char *)NULL) {
                G.rootlen = 0;
                return MPN_NOMEM;
            }
            strcpy(tmproot, pathcomp);
            if (isalpha((uch)tmproot[0]) && tmproot[1] == ':')
                has_drive = TRUE;   /* drive designator */
            if (tmproot[G.rootlen-1] == '/' || tmproot[G.rootlen-1] == '\\') {
                tmproot[--G.rootlen] = '\0';
                had_trailing_pathsep = TRUE;
            }
            if (has_drive && (G.rootlen == 2)) {
                if (!had_trailing_pathsep)   /* i.e., original wasn't "x:/" */
                    add_dot = TRUE;    /* relative path: add '.' before '/' */
            } else if (G.rootlen > 0) {   /* need not check "x:." and "x:/" */
                if (SSTAT(tmproot, &G.statbuf) || !S_ISDIR(G.statbuf.st_mode))
                {
                    /* path does not exist */
                    if (!G.create_dirs /* || iswild(tmproot) */ ) {
                        free(tmproot);
                        G.rootlen = 0;
                        /* treat as stored file */
                        return MPN_INF_SKIP;
                    }
                    /* create directory (could add loop here scanning tmproot
                     * to create more than one level, but really necessary?) */
                    if (MKDIR(tmproot, 0777) == -1) {
                        Info(slide, 1, ((char *)slide,
                          "checkdir:  cannot create extraction directory: %s\n",
                          FnFilter1(tmproot)));
                        free(tmproot);
                        G.rootlen = 0;
                        /* path didn't exist, tried to create, failed: */
                        /* file exists, or need 2+ subdir levels */
                        return MPN_ERR_SKIP;
                    }
                }
            }
            if (add_dot)                    /* had just "x:", make "x:." */
                tmproot[G.rootlen++] = '.';
            tmproot[G.rootlen++] = '/';
            tmproot[G.rootlen] = '\0';
            if ((G.rootpath = (char *)realloc(tmproot, G.rootlen+1)) == NULL) {
                free(tmproot);
                G.rootlen = 0;
                return MPN_NOMEM;
            }
            Trace((stderr, "rootpath now = [%s]\n", FnFilter1(G.rootpath)));
        }
        return MPN_OK;
    }
#endif /* !SFX || SFX_EXDIR */

/*---------------------------------------------------------------------------
    END:  free rootpath, immediately prior to program exit.
  ---------------------------------------------------------------------------*/

    if (FUNCTION == END) {
        Trace((stderr, "freeing rootpath\n"));
        if (G.rootlen > 0) {
            free(G.rootpath);
            G.rootlen = 0;
        }
        return MPN_OK;
    }

    return MPN_INVALID; /* should never reach */

} /* end function checkdir() */





#ifndef SFX

/*************************/
/* Function dateformat() */
/*************************/

int dateformat()
{
  char df[2];   /* LOCALE_IDATE has a maximum value of 2 */

  if (GetLocaleInfoA(LOCALE_USER_DEFAULT, LOCALE_IDATE, df, 2) != 0) {
    switch (df[0])
    {
      case '0':
        return DF_MDY;
      case '1':
        return DF_DMY;
      case '2':
        return DF_YMD;
    }
  }
  return DF_MDY;
}


/****************************/
/* Function dateseparator() */
/****************************/

char dateseparator()
{
  char df[2];   /* use only if it is one character */

  if ((GetLocaleInfoA(LOCALE_USER_DEFAULT, LOCALE_SDATE, df, 2) != 0) &&
      (df[0] != '\0'))
    return df[0];
  else
    return '-';
}


#ifndef WINDLL

/************************/
/*  Function version()  */
/************************/

void version(__G)
    __GDEF
{
    int len;
#if (defined(_MSC_VER) || defined(__WATCOMC__) || defined(__DJGPP__))
    char buf[80];
#if (defined(_MSC_VER) && (_MSC_VER > 900))
    char buf2[80];
#endif
#endif

    len = sprintf((char *)slide, CompiledWith,

#if defined(_MSC_VER)  /* MSC == VC++, but what about SDK compiler? */
      (sprintf(buf, "Microsoft C %d.%02d ", _MSC_VER/100, _MSC_VER%100), buf),
#  if (_MSC_VER == 800)
      "(Visual C++ v1.1)",
#  elif (_MSC_VER == 850)
      "(Windows NT v3.5 SDK)",
#  elif (_MSC_VER == 900)
      "(Visual C++ v2.x)",
#  elif (_MSC_VER > 900)
      (sprintf(buf2, "(Visual C++ %d.%d)", _MSC_VER/100 - 6, _MSC_VER%100/10),
        buf2),
#  else
      "(bad version)",
#  endif
#elif defined(__WATCOMC__)
#  if (__WATCOMC__ % 10 > 0)
      (sprintf(buf, "Watcom C/C++ %d.%02d", __WATCOMC__ / 100,
       __WATCOMC__ % 100), buf), "",
#  else
      (sprintf(buf, "Watcom C/C++ %d.%d", __WATCOMC__ / 100,
       (__WATCOMC__ % 100) / 10), buf), "",
#  endif
#elif defined(__BORLANDC__)
      "Borland C++",
#  if (__BORLANDC__ < 0x0200)
      " 1.0",
#  elif (__BORLANDC__ == 0x0200)
      " 2.0",
#  elif (__BORLANDC__ == 0x0400)
      " 3.0",
#  elif (__BORLANDC__ == 0x0410)   /* __TURBOC__ = 0x0310 */
      " 3.1",
#  elif (__BORLANDC__ == 0x0452)   /* __TURBOC__ = 0x0320 */
      " 4.0 or 4.02",
#  elif (__BORLANDC__ == 0x0460)   /* __TURBOC__ = 0x0340 */
      " 4.5",
#  elif (__BORLANDC__ == 0x0500)   /* __TURBOC__ = 0x0340 */
      " 5.0",
#  elif (__BORLANDC__ == 0x0520)   /* __TURBOC__ = 0x0520 */
      " 5.2 (C++ Builder 1.0)",
#  elif (__BORLANDC__ == 0x0530)   /* __TURBOC__ = 0x0530 */
      " 5.3 (C++ Builder 3.0)",
#  elif (__BORLANDC__ == 0x0540)   /* __TURBOC__ = 0x0540 */
      " 5.4 (C++ Builder 4.0)",
#  elif (__BORLANDC__ == 0x0550)   /* __TURBOC__ = 0x0550 */
      " 5.5 (C++ Builder 5.0)",
#  elif (__BORLANDC__ == 0x0551)   /* __TURBOC__ = 0x0551 */
      " 5.5.1 (C++ Builder 5.0.1)",
#  elif (__BORLANDC__ == 0x0560)   /* __TURBOC__ = 0x0560 */
      " 6.0 (C++ Builder 6.0)",
#  else
      " later than 6.0",
#  endif
#elif defined(__LCC__)
      "LCC-Win32", "",
#elif defined(__GNUC__)
#  if defined(__RSXNT__)
#    if (defined(__DJGPP__) && !defined(__EMX__))
      (sprintf(buf, "rsxnt(djgpp v%d.%02d) / gcc ",
        __DJGPP__, __DJGPP_MINOR__), buf),
#    elif defined(__DJGPP__)
      (sprintf(buf, "rsxnt(emx+djgpp v%d.%02d) / gcc ",
        __DJGPP__, __DJGPP_MINOR__), buf),
#    elif (defined(__GO32__) && !defined(__EMX__))
      "rsxnt(djgpp v1.x) / gcc ",
#    elif defined(__GO32__)
      "rsxnt(emx + djgpp v1.x) / gcc ",
#    elif defined(__EMX__)
      "rsxnt(emx)+gcc ",
#    else
      "rsxnt(unknown) / gcc ",
#    endif
#  elif defined(__CYGWIN__)
      "cygnus win32 / gcc ",
#  elif defined(__MINGW32__)
      "mingw32 / gcc ",
#  else
      "gcc ",
#  endif
      __VERSION__,
#else /* !_MSC_VER, !__WATCOMC__, !__BORLANDC__, !__LCC__, !__GNUC__ */
      "unknown compiler (SDK?)", "",
#endif /* ?compilers */

      "\nWindows 9x / Windows NT/2K/XP/2K3", " (32-bit)",

#ifdef __DATE__
      " on ", __DATE__
#else
      "", ""
#endif
    );

    (*G.message)((zvoid *)&G, slide, (ulg)len, 0);

    return;

} /* end function version() */

#endif /* !WINDLL */
#endif /* !SFX */



#ifdef MORE

int screensize(int *tt_rows, int *tt_cols)
{
    HANDLE hstdout;
    CONSOLE_SCREEN_BUFFER_INFO scr;

    hstdout = GetStdHandle(STD_OUTPUT_HANDLE);
    GetConsoleScreenBufferInfo(hstdout, &scr);
    if (tt_rows != NULL) *tt_rows = scr.srWindow.Bottom - scr.srWindow.Top + 1;
    if (tt_cols != NULL) *tt_cols = scr.srWindow.Right - scr.srWindow.Left + 1;
    return 0;           /* signal success */
}

#endif /* MORE */



#ifdef W32_STAT_BANDAID

/* All currently known variants of WIN32 operating systems (Windows 95/98,
 * WinNT 3.x, 4.0, 5.x) have a nasty bug in the OS kernel concerning
 * conversions between UTC and local time: In the time conversion functions
 * of the Win32 API, the timezone offset (including seasonal daylight saving
 * shift) between UTC and local time evaluation is erratically based on the
 * current system time. The correct evaluation must determine the offset
 * value as it {was/is/will be} for the actual time to be converted.
 *
 * Newer versions of MS C runtime lib's stat() returns utc time-stamps so
 * that localtime(timestamp) corresponds to the (potentially false) local
 * time shown by the OS' system programs (Explorer, command shell dir, etc.)
 * The RSXNT port follows the same strategy, but fails to recognize the
 * access-time attribute.
 *
 * For the NTFS file system (and other filesystems that store time-stamps
 * as UTC values), this results in st_mtime (, st_{c|a}time) fields which
 * are not stable but vary according to the seasonal change of "daylight
 * saving time in effect / not in effect".
 *
 * Other C runtime libs (CygWin), or the crtdll.dll supplied with Win9x/NT
 * return the unix-time equivalent of the UTC FILETIME values as got back
 * from the Win32 API call. This time, return values from NTFS are correct
 * whereas utimes from files on (V)FAT volumes vary according to the DST
 * switches.
 *
 * To achieve timestamp consistency of UTC (UT extra field) values in
 * Zip archives, the Info-ZIP programs require work-around code for
 * proper time handling in stat() (and other time handling routines).
 *
 * However, nowadays most other programs on Windows systems use the
 * time conversion strategy of Microsofts C runtime lib "msvcrt.dll".
 * To improve interoperability in environments where a "consistent" (but
 * false) "UTC<-->LocalTime" conversion is preferred over "stable" time
 * stamps, the Info-ZIP specific time conversion handling can be
 * deactivated by defining the preprocessor flag NO_W32TIMES_IZFIX.
 */
/* stat() functions under Windows95 tend to fail for root directories.   *
 * Watcom and Borland, at least, are affected by this bug.  Watcom made  *
 * a partial fix for 11.0 but still missed some cases.  This substitute  *
 * detects the case and fills in reasonable values.  Otherwise we get    *
 * effects like failure to extract to a root dir because it's not found. */

int zstat_win32(__W32STAT_GLOBALS__ const char *path, z_stat *buf)
{
    if (!zstat(path, buf))
    {
        /* stat was successful, now redo the time-stamp fetches */
#ifndef NO_W32TIMES_IZFIX
        int fs_uses_loctime = FStampIsLocTime(__G__ path);
#endif
        HANDLE h;
        FILETIME Modft, Accft, Creft;
#ifdef __RSXNT__        /* RSXNT/EMX C rtl uses OEM charset */
        char *ansi_path = (char *)alloca(strlen(path) + 1);

        INTERN_TO_ISO(path, ansi_path);
#       define Ansi_Path  ansi_path
#else
#       define Ansi_Path  path
#endif

        TTrace((stdout, "stat(%s) finds modtime %08lx\n", path, buf->st_mtime));
        h = CreateFileA(Ansi_Path, GENERIC_READ,
                        FILE_SHARE_READ | FILE_SHARE_WRITE, NULL,
                        OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, NULL);
        if (h != INVALID_HANDLE_VALUE) {
            BOOL ftOK = GetFileTime(h, &Creft, &Accft, &Modft);
            CloseHandle(h);

            if (ftOK) {
                FTTrace((stdout, "GetFileTime returned Modft", 0, &Modft));
                FTTrace((stdout, "GetFileTime returned Creft", 0, &Creft));
#ifndef NO_W32TIMES_IZFIX
                if (!fs_uses_loctime) {
                    /*  On a filesystem that stores UTC timestamps, we refill
                     *  the time fields of the struct stat buffer by directly
                     *  using the UTC values as returned by the Win32
                     *  GetFileTime() API call.
                     */
                    NtfsFileTime2utime(&Modft, &(buf->st_mtime));
                    if (Accft.dwLowDateTime != 0 || Accft.dwHighDateTime != 0)
                        NtfsFileTime2utime(&Accft, &(buf->st_atime));
                    else
                        buf->st_atime = buf->st_mtime;
                    if (Creft.dwLowDateTime != 0 || Creft.dwHighDateTime != 0)
                        NtfsFileTime2utime(&Creft, &(buf->st_ctime));
                    else
                        buf->st_ctime = buf->st_mtime;
                    TTrace((stdout,"NTFS, recalculated modtime %08lx\n",
                            buf->st_mtime));
                } else
#endif /* NO_W32TIMES_IZFIX */
                {
                    /*  On VFAT and FAT-like filesystems, the FILETIME values
                     *  are converted back to the stable local time before
                     *  converting them to UTC unix time-stamps.
                     */
                    VFatFileTime2utime(&Modft, &(buf->st_mtime));
                    if (Accft.dwLowDateTime != 0 || Accft.dwHighDateTime != 0)
                        VFatFileTime2utime(&Accft, &(buf->st_atime));
                    else
                        buf->st_atime = buf->st_mtime;
                    if (Creft.dwLowDateTime != 0 || Creft.dwHighDateTime != 0)
                        VFatFileTime2utime(&Creft, &(buf->st_ctime));
                    else
                        buf->st_ctime = buf->st_mtime;
                    TTrace((stdout, "VFAT, recalculated modtime %08lx\n",
                            buf->st_mtime));
                }
            }
        }
#       undef Ansi_Path
        return 0;
    }
#ifdef W32_STATROOT_FIX
    else
    {
        DWORD flags;
#ifdef __RSXNT__        /* RSXNT/EMX C rtl uses OEM charset */
        char *ansi_path = (char *)alloca(strlen(path) + 1);

        INTERN_TO_ISO(path, ansi_path);
#       define Ansi_Path  ansi_path
#else
#       define Ansi_Path  path
#endif

        flags = GetFileAttributesA(Ansi_Path);
        if (flags != 0xFFFFFFFF && flags & FILE_ATTRIBUTE_DIRECTORY) {
            Trace((stderr, "\nstat(\"%s\",...) failed on existing directory\n",
                   FnFilter1(path)));
            memset(buf, 0, sizeof(z_stat));
            buf->st_atime = buf->st_ctime = buf->st_mtime =
              dos_to_unix_time(DOSTIME_MINIMUM);        /* 1-1-80 */
            buf->st_mode = S_IFDIR | S_IREAD |
                           ((flags & FILE_ATTRIBUTE_READONLY) ? 0 : S_IWRITE);
            return 0;
        } /* assumes: stat() won't fail on non-dirs without good reason */
#       undef Ansi_Path
    }
#endif /* W32_STATROOT_FIX */
    return -1;
}

#endif /* W32_STAT_BANDAID */



#ifdef W32_USE_IZ_TIMEZONE
#include "timezone.h"
#define SECSPERMIN      60
#define MINSPERHOUR     60
#define SECSPERHOUR     (SECSPERMIN * MINSPERHOUR)
static void conv_to_rule(LPSYSTEMTIME lpw32tm, struct rule * ZCONST ptrule);

static void conv_to_rule(LPSYSTEMTIME lpw32tm, struct rule * ZCONST ptrule)
{
    if (lpw32tm->wYear != 0) {
        ptrule->r_type = JULIAN_DAY;
        ptrule->r_day = ydays[lpw32tm->wMonth - 1] + lpw32tm->wDay;
    } else {
        ptrule->r_type = MONTH_NTH_DAY_OF_WEEK;
        ptrule->r_mon = lpw32tm->wMonth;
        ptrule->r_day = lpw32tm->wDayOfWeek;
        ptrule->r_week = lpw32tm->wDay;
    }
    ptrule->r_time = (long)lpw32tm->wHour * SECSPERHOUR +
                     (long)(lpw32tm->wMinute * SECSPERMIN) +
                     (long)lpw32tm->wSecond;
}

int GetPlatformLocalTimezone(register struct state * ZCONST sp,
        void (*fill_tzstate_from_rules)(struct state * ZCONST sp_res,
                                        ZCONST struct rule * ZCONST start,
                                        ZCONST struct rule * ZCONST end))
{
    TIME_ZONE_INFORMATION tzinfo;
    DWORD res;

    /* read current timezone settings from registry if TZ envvar missing */
    res = GetTimeZoneInformation(&tzinfo);
    if (res != TIME_ZONE_ID_INVALID)
    {
        struct rule startrule, stoprule;

        conv_to_rule(&(tzinfo.StandardDate), &stoprule);
        conv_to_rule(&(tzinfo.DaylightDate), &startrule);
        sp->timecnt = 0;
        sp->ttis[0].tt_abbrind = 0;
        if ((sp->charcnt =
             WideCharToMultiByte(CP_ACP, 0, tzinfo.StandardName, -1,
                                 sp->chars, sizeof(sp->chars), NULL, NULL))
            == 0)
            sp->chars[sp->charcnt++] = '\0';
        sp->ttis[1].tt_abbrind = sp->charcnt;
        sp->charcnt +=
            WideCharToMultiByte(CP_ACP, 0, tzinfo.DaylightName, -1,
                                sp->chars + sp->charcnt,
                                sizeof(sp->chars) - sp->charcnt, NULL, NULL);
        if ((sp->charcnt - sp->ttis[1].tt_abbrind) == 0)
            sp->chars[sp->charcnt++] = '\0';
        sp->ttis[0].tt_gmtoff = - (tzinfo.Bias + tzinfo.StandardBias)
                                * MINSPERHOUR;
        sp->ttis[1].tt_gmtoff = - (tzinfo.Bias + tzinfo.DaylightBias)
                                * MINSPERHOUR;
        sp->ttis[0].tt_isdst = 0;
        sp->ttis[1].tt_isdst = 1;
        sp->typecnt = (startrule.r_mon == 0 && stoprule.r_mon == 0) ? 1 : 2;

        if (sp->typecnt > 1)
            (*fill_tzstate_from_rules)(sp, &startrule, &stoprule);
        return TRUE;
    }
    return FALSE;
}
#endif /* W32_USE_IZ_TIMEZONE */

#endif /* !FUNZIP */



#ifndef WINDLL
/* This replacement getch() function was originally created for Watcom C
 * and then additionally used with CYGWIN. Since UnZip 5.4, all other Win32
 * ports apply this replacement rather that their supplied getch() (or
 * alike) function.  There are problems with unabsorbed LF characters left
 * over in the keyboard buffer under Win95 (and 98) when ENTER was pressed.
 * (Under Win95, ENTER returns two(!!) characters: CR-LF.)  This problem
 * does not appear when run on a WinNT console prompt!
 */

/* Watcom 10.6's getch() does not handle Alt+<digit><digit><digit>. */
/* Note that if PASSWD_FROM_STDIN is defined, the file containing   */
/* the password must have a carriage return after the word, not a   */
/* Unix-style newline (linefeed only).  This discards linefeeds.    */

int getch_win32(void)
{
  HANDLE stin;
  DWORD rc;
  unsigned char buf[2];
  int ret = -1;
  DWORD odemode = ~(DWORD)0;

#  ifdef PASSWD_FROM_STDIN
  stin = GetStdHandle(STD_INPUT_HANDLE);
#  else
  stin = CreateFileA("CONIN$", GENERIC_READ | GENERIC_WRITE,
                     FILE_SHARE_READ, NULL, OPEN_EXISTING, 0, NULL);
  if (stin == INVALID_HANDLE_VALUE)
    return -1;
#  endif
  if (GetConsoleMode(stin, &odemode))
    SetConsoleMode(stin, ENABLE_PROCESSED_INPUT);  /* raw except ^C noticed */
  if (ReadFile(stin, &buf, 1, &rc, NULL) && rc == 1)
    ret = buf[0];
  /* when the user hits return we get CR LF.  We discard the LF, not the CR,
   * because when we call this for the first time after a previous input
   * such as the one for "replace foo? [y]es, ..." the LF may still be in
   * the input stream before whatever the user types at our prompt. */
  if (ret == '\n')
    if (ReadFile(stin, &buf, 1, &rc, NULL) && rc == 1)
      ret = buf[0];
  if (odemode != ~(DWORD)0)
    SetConsoleMode(stin, odemode);
#  ifndef PASSWD_FROM_STDIN
  CloseHandle(stin);
#  endif
  return ret;
}
#endif /* !WINDLL */



#if (defined(UNICODE_SUPPORT) && !defined(FUNZIP))
/* convert wide character string to multi-byte character string */
char *wide_to_local_string(wide_string, escape_all)
  ZCONST zwchar *wide_string;
  int escape_all;
{
  int i;
  wchar_t wc;
  int bytes_char;
  int default_used;
  int wsize = 0;
  int max_bytes = 9;
  char buf[9];
  char *buffer = NULL;
  char *local_string = NULL;

  for (wsize = 0; wide_string[wsize]; wsize++) ;

  if (max_bytes < MB_CUR_MAX)
    max_bytes = MB_CUR_MAX;

  if ((buffer = (char *)malloc(wsize * max_bytes + 1)) == NULL) {
    return NULL;
  }

  /* convert it */
  buffer[0] = '\0';
  for (i = 0; i < wsize; i++) {
    if (sizeof(wchar_t) < 4 && wide_string[i] > 0xFFFF) {
      /* wchar_t probably 2 bytes */
      /* could do surrogates if state_dependent and wctomb can do */
      wc = zwchar_to_wchar_t_default_char;
    } else {
      wc = (wchar_t)wide_string[i];
    }
    /* Unter some vendor's C-RTL, the Wide-to-MultiByte conversion functions
     * (like wctomb() et. al.) do not use the same codepage as the other
     * string arguments I/O functions (fopen, mkdir, rmdir etc.).
     * Therefore, we have to fall back to the underlying Win32-API call to
     * achieve a consistent behaviour for all supported compiler environments.
     * Failing RTLs are for example:
     *   Borland (locale uses OEM-CP as default, but I/O functions expect ANSI
     *            names)
     *   Watcom  (only "C" locale, wctomb() always uses OEM CP)
     * (in other words: all supported environments except the Microsoft RTLs)
     */
    bytes_char = WideCharToMultiByte(
                          CP_ACP, WC_COMPOSITECHECK,
                          &wc, 1,
                          (LPSTR)buf, sizeof(buf),
                          NULL, &default_used);
    if (default_used)
      bytes_char = -1;
    if (escape_all) {
      if (bytes_char == 1 && (uch)buf[0] <= 0x7f) {
        /* ASCII */
        strncat(buffer, buf, 1);
      } else {
        /* use escape for wide character */
        char *escape_string = wide_to_escape_string(wide_string[i]);
        strcat(buffer, escape_string);
        free(escape_string);
      }
    } else if (bytes_char > 0) {
      /* multi-byte char */
      strncat(buffer, buf, bytes_char);
    } else {
      /* no MB for this wide */
      /* use escape for wide character */
      char *escape_string = wide_to_escape_string(wide_string[i]);
      strcat(buffer, escape_string);
      free(escape_string);
    }
  }
  if ((local_string = (char *)realloc(buffer, strlen(buffer) + 1)) == NULL) {
    free(buffer);
    return NULL;
  }

  return local_string;
}


#if 0
wchar_t *utf8_to_wchar_string(utf8_string)
  char *utf8_string;       /* path to get utf-8 name for */
{
  wchar_t  *qw;
  int       ulen;
  int       ulenw;

  if (utf8_string == NULL)
    return NULL;

    /* get length */
    ulenw = MultiByteToWideChar(
                CP_UTF8,           /* UTF-8 code page */
                0,                 /* flags for character-type options */
                utf8_string,       /* string to convert */
                -1,                /* string length (-1 = NULL terminated) */
                NULL,              /* buffer */
                0 );               /* buffer length (0 = return length) */
    if (ulenw == 0) {
      /* failed */
      return NULL;
    }
    ulenw++;
    /* get length in bytes */
    ulen = sizeof(wchar_t) * (ulenw + 1);
    if ((qw = (wchar_t *)malloc(ulen + 1)) == NULL) {
      return NULL;
    }
    /* convert multibyte to wide */
    ulen = MultiByteToWideChar(
               CP_UTF8,           /* UTF-8 code page */
               0,                 /* flags for character-type options */
               utf8_string,       /* string to convert */
               -1,                /* string length (-1 = NULL terminated) */
               qw,                /* buffer */
               ulenw);            /* buffer length (0 = return length) */
    if (ulen == 0) {
      /* failed */
      free(qw);
      return NULL;
    }

  return qw;
}

wchar_t *local_to_wchar_string(local_string)
  char *local_string;       /* path of local name */
{
  wchar_t  *qw;
  int       ulen;
  int       ulenw;

  if (local_string == NULL)
    return NULL;

    /* get length */
    ulenw = MultiByteToWideChar(
                CP_ACP,            /* ANSI code page */
                0,                 /* flags for character-type options */
                local_string,      /* string to convert */
                -1,                /* string length (-1 = NULL terminated) */
                NULL,              /* buffer */
                0 );               /* buffer length (0 = return length) */
    if (ulenw == 0) {
      /* failed */
      return NULL;
    }
    ulenw++;
    /* get length in bytes */
    ulen = sizeof(wchar_t) * (ulenw + 1);
    if ((qw = (wchar_t *)malloc(ulen + 1)) == NULL) {
      return NULL;
    }
    /* convert multibyte to wide */
    ulen = MultiByteToWideChar(
               CP_ACP,            /* ANSI code page */
               0,                 /* flags for character-type options */
               local_string,      /* string to convert */
               -1,                /* string length (-1 = NULL terminated) */
               qw,                /* buffer */
               ulenw);            /* buffer length (0 = return length) */
    if (ulen == 0) {
      /* failed */
      free(qw);
      return NULL;
    }

  return qw;
}
#endif /* 0 */
#endif /* UNICODE_SUPPORT && !FUNZIP */



/* --------------------------------------------------- */
/* Large File Support
 *
 * Initial functions by E. Gordon and R. Nausedat
 * 9/10/2003
 * Lifted from Zip 3b, win32.c and place here by Myles Bennett
 * 7/6/2004
 *
 * These implement 64-bit file support for Windows.  The
 * defines and headers are in win32/w32cfg.h.
 *
 * Moved to win32i64.c by Mike White to avoid conflicts in
 * same name functions in WiZ using UnZip and Zip libraries.
 * 9/25/2003
 */
