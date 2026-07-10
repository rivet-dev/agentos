/*
  Copyright (c) 1990-2008 Info-ZIP.  All rights reserved.

  See the accompanying file LICENSE, version 2000-Apr-09 or later
  (the contents of which are also included in unzip.h) for terms of use.
  If, for some reason, all these files are missing, the Info-ZIP license
  also may be found at:  ftp://ftp.info-zip.org/pub/infozip/license.html
*/
/*---------------------------------------------------------------------------

  msdos.c

  MSDOS-specific routines for use with Info-ZIP's UnZip 5.3 and later.

  Contains:  Opendir()                      (from zip)
             Readdir()                      (from zip)
             do_wild()
             mapattr()
             mapname()
             maskDOSdevice()
             map2fat()
             checkdir()
             isfloppy()
             z_dos_chmod()
             volumelabel()                  (non-djgpp, non-emx)
             close_outfile()
             stamp_file()                   (TIMESTAMP only)
             prepare_ISO_OEM_translat()
             dateformat()
             version()
             zcalloc()                      (16-bit, only)
             zcfree()                       (16-bit, only)
             _dos_getcountryinfo()          (djgpp 1.x, emx)
            [_dos_getftime()                (djgpp 1.x, emx)   to be added]
             _dos_setftime()                (djgpp 1.x, emx)
             _dos_setfileattr()             (djgpp 1.x, emx)
             _dos_getdrive()                (djgpp 1.x, emx)
             _dos_creat()                   (djgpp 1.x, emx)
             _dos_close()                   (djgpp 1.x, emx)
             volumelabel()                  (djgpp, emx)
             _dos_getcountryinfo()          (djgpp 2.x)
             _is_executable()               (djgpp 2.x)
             __crt0_glob_function()         (djgpp 2.x)
             __crt0_load_environment_file() (djgpp 2.x)
             dos_getcodepage()              (all, ASM system call)
             screensize()                   (emx, Watcom 32-bit)
             int86x_realmode()              (Watcom 32-bit)
             stat_bandaid()                 (Watcom)

  ---------------------------------------------------------------------------*/



#define UNZIP_INTERNAL
#include "unzip.h"

/* fUnZip does not need anything from here except the zcalloc() & zcfree()
 * function pair (when Deflate64 support is enabled in 16-bit environment).
 */
#ifndef FUNZIP

static void maskDOSdevice(__GPRO__ char *pathcomp, char *last_dot);
#ifdef MAYBE_PLAIN_FAT
   static void map2fat OF((char *pathcomp, char *last_dot));
#endif
static int isfloppy OF((int nDrive));
static int z_dos_chmod OF((__GPRO__ ZCONST char *fname, int attributes));
static int volumelabel OF((ZCONST char *newlabel));
#if (!defined(SFX) && !defined(WINDLL))
   static int is_running_on_windows OF((void));
#endif
static int getdoscodepage OF((void));

static int created_dir;        /* used by mapname(), checkdir() */
static int renamed_fullpath;   /* ditto */
static unsigned nLabelDrive;   /* ditto, plus volumelabel() */



/*****************************/
/*  Strings used in msdos.c  */
/*****************************/

#ifndef SFX
  static ZCONST char Far CantAllocateWildcard[] =
    "warning:  cannot allocate wildcard buffers\n";
#endif
static ZCONST char Far WarnDirTraversSkip[] =
  "warning:  skipped \"../\" path component(s) in %s\n";
static ZCONST char Far Creating[] = "   creating: %s\n";
static ZCONST char Far ConversionFailed[] =
  "mapname:  conversion of %s failed\n";
static ZCONST char Far Labelling[] = "labelling %c: %-22s\n";
static ZCONST char Far ErrSetVolLabel[] =
  "mapname:  error setting volume label\n";
static ZCONST char Far PathTooLong[] = "checkdir error:  path too long: %s\n";
static ZCONST char Far CantCreateDir[] = "checkdir error:  cannot create %s\n\
                 unable to process %s.\n";
static ZCONST char Far DirIsntDirectory[] =
  "checkdir error:  %s exists but is not directory\n\
                 unable to process %s.\n";
static ZCONST char Far PathTooLongTrunc[] =
  "checkdir warning:  path too long; truncating\n                   %s\n\
                -> %s\n";
#if (!defined(SFX) || defined(SFX_EXDIR))
   static ZCONST char Far CantCreateExtractDir[] =
     "checkdir:  cannot create extraction directory: %s\n";
#endif
static ZCONST char Far AttribsMayBeWrong[] =
  "\nwarning:  file attributes may not be correct\n";
#if (!defined(SFX) && !defined(WINDLL))
   static ZCONST char Far WarnUsedOnWindows[] =
     "\n%s warning: You are using the MSDOS version on Windows.\n"
     "Please try the native Windows version before reporting any problems.\n";
#endif



/****************************/
/*  Macros used in msdos.c  */
/****************************/

#ifdef WATCOMC_386
#  define WREGS(v,r) (v.w.r)
#  define int86x int386x
   static int int86x_realmode(int inter_no, union REGS *in,
                              union REGS *out, struct SREGS *seg);
#  define F_intdosx(ir,or,sr) int86x_realmode(0x21, ir, or, sr)
#  define XXX__MK_FP_IS_BROKEN
#else
#  if (defined(__DJGPP__) && (__DJGPP__ >= 2))
#   define WREGS(v,r) (v.w.r)
#  else
#   define WREGS(v,r) (v.x.r)
#  endif
#  define F_intdosx(ir,or,sr) intdosx(ir, or, sr)
#endif

#if (defined(__GO32__) || defined(__EMX__))
#  include <dirent.h>        /* use readdir() */
#  define MKDIR(path,mode)   mkdir(path,mode)
#  define Opendir  opendir
#  define Readdir  readdir
#  define Closedir closedir
#  define zdirent  dirent
#  define zDIR     DIR
#  ifdef __EMX__
#    include <dos.h>
#    define GETDRIVE(d)      d = _getdrive()
#    define FA_LABEL         A_LABEL
#  else
#    define GETDRIVE(d)      _dos_getdrive(&d)
#  endif
#  if defined(_A_SUBDIR)     /* MSC dos.h and compatibles */
#    define FSUBDIR          _A_SUBDIR
#  elif defined(FA_DIREC)    /* Borland dos.h and compatible variants */
#    define FSUBDIR          FA_DIREC
#  elif defined(A_DIR)       /* EMX dir.h (and dirent.h) */
#    define FSUBDIR          A_DIR
#  else                      /* fallback definition */
#    define FSUBDIR          0x10
#  endif
#  if defined(_A_VOLID)      /* MSC dos.h and compatibles */
#    define FVOLID           _A_VOLID
#  elif defined(FA_LABEL)    /* Borland dos.h and compatible variants */
#    define FVOLID           FA_LABEL
#  elif defined(A_LABEL)     /* EMX dir.h (and dirent.h) */
#    define FVOLID           A_LABEL
#  else
#    define FVOLID           0x08
#  endif
#else /* !(__GO32__ || __EMX__) */
#  define MKDIR(path,mode)   mkdir(path)
#  ifdef __TURBOC__
#    define FATTR            FA_HIDDEN+FA_SYSTEM+FA_DIREC
#    define FVOLID           FA_LABEL
#    define FSUBDIR          FA_DIREC
#    define FFIRST(n,d,a)    findfirst(n,(struct ffblk *)d,a)
#    define FNEXT(d)         findnext((struct ffblk *)d)
#    define GETDRIVE(d)      d=getdisk()+1
#    include <dir.h>
#  else /* !__TURBOC__ */
#    define FATTR            _A_HIDDEN+_A_SYSTEM+_A_SUBDIR
#    define FVOLID           _A_VOLID
#    define FSUBDIR          _A_SUBDIR
#    define FFIRST(n,d,a)    _dos_findfirst(n,a,(struct find_t *)d)
#    define FNEXT(d)         _dos_findnext((struct find_t *)d)
#    define GETDRIVE(d)      _dos_getdrive(&d)
#    include <direct.h>
#  endif /* ?__TURBOC__ */
   typedef struct zdirent {
       char d_reserved[30];
       char d_name[13];
       int d_first;
   } zDIR;
   zDIR *Opendir OF((const char *));
   struct zdirent *Readdir OF((zDIR *));
#  define Closedir free




#ifndef SFX

/**********************/   /* Borland C++ 3.x has its own opendir/readdir */
/* Function Opendir() */   /*  library routines, but earlier versions don't, */
/**********************/   /*  so use ours regardless */

zDIR *Opendir(name)
    const char *name;           /* name of directory to open */
{
    zDIR *dirp;                 /* malloc'd return value */
    char *nbuf;                 /* malloc'd temporary string */
    extent len = strlen(name);  /* path length to avoid strlens and strcats */


    if ((dirp = (zDIR *)malloc(sizeof(zDIR))) == (zDIR *)NULL)
        return (zDIR *)NULL;
    if ((nbuf = malloc(len + 6)) == (char *)NULL) {
        free(dirp);
        return (zDIR *)NULL;
    }
    strcpy(nbuf, name);
    if (len > 0) {
        if (nbuf[len-1] == ':') {
            nbuf[len++] = '.';
        } else if (nbuf[len-1] == '/' || nbuf[len-1] == '\\')
            --len;
    }
    strcpy(nbuf+len, "/*.*");
    Trace((stderr, "Opendir:  nbuf = [%s]\n", FnFilter1(nbuf)));

    if (FFIRST(nbuf, dirp, FATTR)) {
        free((zvoid *)nbuf);
        return (zDIR *)NULL;
    }
    free((zvoid *)nbuf);
    dirp->d_first = 1;
    return dirp;
}





/**********************/
/* Function Readdir() */
/**********************/

struct zdirent *Readdir(d)
    zDIR *d;        /* directory stream from which to read */
{
    /* Return pointer to first or next directory entry, or NULL if end. */

    if (d->d_first)
        d->d_first = 0;
    else
        if (FNEXT(d))
            return (struct zdirent *)NULL;
    return (struct zdirent *)d;
}

#endif /* !SFX */
#endif /* ?(__GO32__ || __EMX__) */





#ifndef SFX

/************************/
/*  Function do_wild()  */   /* identical to OS/2 version */
/************************/

char *do_wild(__G__ wildspec)
    __GDEF
    ZCONST char *wildspec;   /* only used first time on a given dir */
{
    static zDIR *wild_dir = (zDIR *)NULL;
    static ZCONST char *wildname;
    static char *dirname, matchname[FILNAMSIZ];
    static int notfirstcall=FALSE, have_dirname, dirnamelen;
    char *fnamestart;
    struct zdirent *file;

    /* Even when we're just returning wildspec, we *always* do so in
     * matchname[]--calling routine is allowed to append four characters
     * to the returned string, and wildspec may be a pointer to argv[].
     */
    if (!notfirstcall) {    /* first call:  must initialize everything */
        notfirstcall = TRUE;

        if (!iswild(wildspec)) {
            strncpy(matchname, wildspec, FILNAMSIZ);
            matchname[FILNAMSIZ-1] = '\0';
            have_dirname = FALSE;
            wild_dir = NULL;
            return matchname;
        }

        /* break the wildspec into a directory part and a wildcard filename */
        if ((wildname = strrchr(wildspec, '/')) == (ZCONST char *)NULL &&
            (wildname = strrchr(wildspec, ':')) == (ZCONST char *)NULL) {
            dirname = ".";
            dirnamelen = 1;
            have_dirname = FALSE;
            wildname = wildspec;
        } else {
            ++wildname;     /* point at character after '/' or ':' */
            dirnamelen = (int)(wildname - wildspec);
            if ((dirname = (char *)malloc(dirnamelen+1)) == (char *)NULL) {
                Info(slide, 1, ((char *)slide,
                  LoadFarString(CantAllocateWildcard)));
                strncpy(matchname, wildspec, FILNAMSIZ);
                matchname[FILNAMSIZ-1] = '\0';
                return matchname;   /* but maybe filespec was not a wildcard */
            }
/* GRR:  can't strip trailing char for opendir since might be "d:/" or "d:"
 *       (would have to check for "./" at end--let opendir handle it instead) */
            strncpy(dirname, wildspec, dirnamelen);
            dirname[dirnamelen] = '\0';   /* terminate for strcpy below */
            have_dirname = TRUE;
        }
        Trace((stderr, "do_wild:  dirname = [%s]\n", FnFilter1(dirname)));

        if ((wild_dir = Opendir(dirname)) != (zDIR *)NULL) {
            if (have_dirname) {
                strcpy(matchname, dirname);
                fnamestart = matchname + dirnamelen;
            } else
                fnamestart = matchname;
            while ((file = Readdir(wild_dir)) != (struct zdirent *)NULL) {
                Trace((stderr, "do_wild:  readdir returns %s\n",
                  FnFilter1(file->d_name)));
                strcpy(fnamestart, file->d_name);
                if (strrchr(fnamestart, '.') == (char *)NULL)
                    strcat(fnamestart, ".");
                /* 1 == ignore case (for case-insensitive DOS-FS) */
                if (match(fnamestart, wildname, 1 WISEP) &&
                    /* skip "." and ".." directory entries */
                    strcmp(fnamestart, ".") && strcmp(fnamestart, "..")) {
                    Trace((stderr, "do_wild:  match() succeeds\n"));
                    /* remove trailing dot */
                    fnamestart += strlen(fnamestart) - 1;
                    if (*fnamestart == '.')
                        *fnamestart = '\0';
                    return matchname;
                }
            }
            /* if we get to here directory is exhausted, so close it */
            Closedir(wild_dir);
            wild_dir = (zDIR *)NULL;
        }
#ifdef DEBUG
        else {
            Trace((stderr, "do_wild:  Opendir(%s) returns NULL\n",
               FnFilter1(dirname)));
        }
#endif /* DEBUG */

        /* return the raw wildspec in case that works (e.g., directory not
         * searchable, but filespec was not wild and file is readable) */
        strncpy(matchname, wildspec, FILNAMSIZ);
        matchname[FILNAMSIZ-1] = '\0';
        return matchname;
    }

    /* last time through, might have failed opendir but returned raw wildspec */
    if (wild_dir == (zDIR *)NULL) {
        notfirstcall = FALSE; /* nothing left to try--reset for new wildspec */
        if (have_dirname)
            free(dirname);
        return (char *)NULL;
    }

    /* If we've gotten this far, we've read and matched at least one entry
     * successfully (in a previous call), so dirname has been copied into
     * matchname already.
     */
    if (have_dirname) {
        /* strcpy(matchname, dirname); */
        fnamestart = matchname + dirnamelen;
    } else
        fnamestart = matchname;
    while ((file = Readdir(wild_dir)) != (struct zdirent *)NULL) {
        Trace((stderr, "do_wild:  readdir returns %s\n",
          FnFilter1(file->d_name)));
        strcpy(fnamestart, file->d_name);
        if (strrchr(fnamestart, '.') == (char *)NULL)
            strcat(fnamestart, ".");
        if (match(fnamestart, wildname, 1 WISEP)) { /* 1 == ignore case */
            Trace((stderr, "do_wild:  match() succeeds\n"));
            /* remove trailing dot */
            fnamestart += strlen(fnamestart) - 1;
            if (*fnamestart == '.')
                *fnamestart = '\0';
            return matchname;
        }
    }

    Closedir(wild_dir);     /* have read at least one entry; nothing left */
    wild_dir = (zDIR *)NULL;
    notfirstcall = FALSE;   /* reset for new wildspec */
    if (have_dirname)
        free(dirname);
    return (char *)NULL;

} /* end function do_wild() */

#endif /* !SFX */




/**********************/
/* Function mapattr() */
/**********************/

int mapattr(__G)
    __GDEF
{
    /* set archive bit for file entries (file is not backed up): */
    G.pInfo->file_attr = ((unsigned)G.crec.external_file_attributes |
      (G.crec.external_file_attributes & FSUBDIR ? 0 : 32)) & 0xff;
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
    char pathcomp[FILNAMSIZ];      /* path-component buffer */
    char *pp, *cp=(char *)NULL;    /* character pointers */
    char *lastsemi=(char *)NULL;   /* pointer to last semi-colon in pathcomp */
#ifdef MAYBE_PLAIN_FAT
    char *last_dot=(char *)NULL;   /* last dot not converted to underscore */
# ifdef USE_LFN
    int use_lfn = USE_LFN;         /* file system supports long filenames? */
# endif
#endif
    int killed_ddot = FALSE;       /* is set when skipping "../" pathcomp */
    int error = MPN_OK;
    register unsigned workch;      /* hold the character being tested */


/*---------------------------------------------------------------------------
    Initialize various pointers and counters and stuff.
  ---------------------------------------------------------------------------*/

    /* can create path as long as not just freshening, or if user told us */
    G.create_dirs = (!uO.fflag || renamed);

    created_dir = FALSE;        /* not yet */
    renamed_fullpath = FALSE;

    if (renamed) {
        cp = G.filename - 1;    /* point to beginning of renamed name... */
        while (*++cp)
            if (*cp == '\\')    /* convert backslashes to forward */
                *cp = '/';
        cp = G.filename;
        /* use temporary rootpath if user gave full pathname */
        if (G.filename[0] == '/') {
            renamed_fullpath = TRUE;
            pathcomp[0] = '/';  /* copy the '/' and terminate */
            pathcomp[1] = '\0';
            ++cp;
        } else if (isalpha((uch)G.filename[0]) && G.filename[1] == ':') {
            renamed_fullpath = TRUE;
            pp = pathcomp;
            *pp++ = *cp++;      /* copy the "d:" (+ '/', possibly) */
            *pp++ = *cp++;
            if (*cp == '/')
                *pp++ = *cp++;  /* otherwise add "./"? */
            *pp = '\0';
        }
    }

    /* pathcomp is ignored unless renamed_fullpath is TRUE: */
    if ((error = checkdir(__G__ pathcomp, INIT)) != 0) /* initialize path buf */
        return error;           /* ...unless no mem or vol label on hard disk */

    *pathcomp = '\0';           /* initialize translation buffer */
    pp = pathcomp;              /* point to translation buffer */
    if (!renamed) {             /* cp already set if renamed */
        if (uO.jflag)           /* junking directories */
            cp = (char *)strrchr(G.filename, '/');
        if (cp == (char *)NULL) /* no '/' or not junking dirs */
            cp = G.filename;    /* point to internal zipfile-member pathname */
        else
            ++cp;               /* point to start of last component of path */
    }

/*---------------------------------------------------------------------------
    Begin main loop through characters in filename.
  ---------------------------------------------------------------------------*/

    while ((workch = (uch)*cp++) != 0) {

        switch (workch) {
            case '/':             /* can assume -j flag not given */
                *pp = '\0';
#ifdef MAYBE_PLAIN_FAT
                maskDOSdevice(__G__ pathcomp, last_dot);
#else
                maskDOSdevice(__G__ pathcomp, NULL);
#endif
#ifdef MAYBE_PLAIN_FAT
# ifdef USE_LFN
                if (!use_lfn)
# endif
                {
                    map2fat(pathcomp, last_dot);   /* 8.3 trunc. (in place) */
                    last_dot = (char *)NULL;
                }
#endif
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

#ifdef MAYBE_PLAIN_FAT
            case '.':
# ifdef USE_LFN
                if (use_lfn) {          /* LFN filenames may contain many */
                    *pp++ = '.';        /*  dots, so simply copy it ... */
                } else
# endif
                if (pp == pathcomp && *cp == '.' && cp[1] == '/') {
                    /* nothing appended yet.., and found "../" */
                    *pp++ = '.';        /*  add first dot, */
                    *pp++ = '.';        /*  second dot, and */
                    ++cp;               /*  skip over to the '/' */
                } else {                /* found dot within path component */
                    last_dot = pp;      /*  point at last dot so far... */
                    *pp++ = '_';        /*  convert to underscore for now */
                }
                break;
#endif /* MAYBE_PLAIN_FAT */

            /* drive names are not stored in zipfile, so no colons allowed;
             *  no brackets or most other punctuation either (all of which
             *  can appear in Unix-created archives; backslash is particularly
             *  bad unless all necessary directories exist) */
#ifdef MAYBE_PLAIN_FAT
            case '[':          /* these punctuation characters forbidden */
            case ']':          /*  only on plain FAT file systems */
            case '+':
            case ',':
            case '=':
# ifdef USE_LFN
                if (use_lfn)
                    *pp++ = (char)workch;
                else
                    *pp++ = '_';
                break;
# endif
#endif
            case ':':           /* special shell characters of command.com */
            case '\\':          /*  (device and directory limiters, wildcard */
            case '"':           /*  characters, stdin/stdout redirection and */
            case '<':           /*  pipe indicators and the quote sign) are */
            case '>':           /*  never allowed in filenames on (V)FAT */
            case '|':
            case '*':
            case '?':
                *pp++ = '_';
                break;

            case ';':             /* start of VMS version? */
                lastsemi = pp;
#ifdef MAYBE_PLAIN_FAT
# ifdef USE_LFN
                if (use_lfn)
                    *pp++ = ';';  /* keep for now; remove VMS ";##" later */
# endif
#else
                *pp++ = ';';      /* keep for now; remove VMS ";##" later */
#endif
                break;

#ifdef MAYBE_PLAIN_FAT
            case ' ':                      /* change spaces to underscores */
# ifdef USE_LFN
                if (!use_lfn && uO.sflag)  /*  only if requested and NO lfn! */
# else
                if (uO.sflag)              /*  only if requested */
# endif
                    *pp++ = '_';
                else
                    *pp++ = (char)workch;
                break;
#endif /* MAYBE_PLAIN_FAT */

            default:
                /* allow ASCII 255 and European characters in filenames: */
                if (isprint(workch) || workch >= 127)
                    *pp++ = (char)workch;

        } /* end switch */
    } /* end while loop */

    /* Show warning when stripping insecure "parent dir" path components */
    if (killed_ddot && QCOND2) {
        Info(slide, 0, ((char *)slide, LoadFarString(WarnDirTraversSkip),
          FnFilter1(G.filename)));
        if (!(error & ~MPN_MASK))
            error = (error & MPN_MASK) | PK_WARN;
    }

/*---------------------------------------------------------------------------
    Report if directory was created (and no file to create:  filename ended
    in '/'), check name to be sure it exists, and combine path and name be-
    fore exiting.
  ---------------------------------------------------------------------------*/

    if (G.filename[strlen(G.filename) - 1] == '/') {
        checkdir(__G__ G.filename, GETPATH);
        if (created_dir) {
            if (QCOND2) {
                Info(slide, 0, ((char *)slide, LoadFarString(Creating),
                  FnFilter1(G.filename)));
            }

            /* set file attributes: */
            z_dos_chmod(__G__ G.filename, G.pInfo->file_attr);

            /* set dir time (note trailing '/') */
            return (error & ~MPN_MASK) | MPN_CREATED_DIR;
        } else if (IS_OVERWRT_ALL) {
            /* overwrite attributes of existing directory on user's request */

            /* set file attributes: */
            z_dos_chmod(__G__ G.filename, G.pInfo->file_attr);
        }
        /* dir existed already; don't look for data to extract */
        return (error & ~MPN_MASK) | MPN_INF_SKIP;
    }

    *pp = '\0';                   /* done with pathcomp:  terminate it */

    /* if not saving them, remove VMS version numbers (appended ";###") */
    if (!uO.V_flag && lastsemi) {
#ifndef MAYBE_PLAIN_FAT
        pp = lastsemi + 1;
#else
# ifdef USE_LFN
        if (use_lfn)
            pp = lastsemi + 1;
        else
            pp = lastsemi;        /* semi-colon was omitted:  expect all #'s */
# else
        pp = lastsemi;            /* semi-colon was omitted:  expect all #'s */
# endif
#endif
        while (isdigit((uch)(*pp)))
            ++pp;
        if (*pp == '\0')          /* only digits between ';' and end:  nuke */
            *lastsemi = '\0';
    }

#ifdef MAYBE_PLAIN_FAT
    maskDOSdevice(__G__ pathcomp, last_dot);
#else
    maskDOSdevice(__G__ pathcomp, NULL);
#endif

    if (G.pInfo->vollabel) {
        if (strlen(pathcomp) > 11)
            pathcomp[11] = '\0';
    } else {
#ifdef MAYBE_PLAIN_FAT
# ifdef USE_LFN
        if (!use_lfn)
            map2fat(pathcomp, last_dot);  /* 8.3 truncation (in place) */
# else
        map2fat(pathcomp, last_dot);  /* 8.3 truncation (in place) */
# endif
#endif
    }

    if (*pathcomp == '\0') {
        Info(slide, 1, ((char *)slide, LoadFarString(ConversionFailed),
          FnFilter1(G.filename)));
        return (error & ~MPN_MASK) | MPN_ERR_SKIP;
    }

    checkdir(__G__ pathcomp, APPEND_NAME);  /* returns 1 if truncated: care? */
    checkdir(__G__ G.filename, GETPATH);

    if (G.pInfo->vollabel) {    /* set the volume label now */
        if (QCOND2)
            Info(slide, 0, ((char *)slide, LoadFarString(Labelling),
              (nLabelDrive + 'a' - 1),
              FnFilter1(G.filename)));
        if (volumelabel(G.filename)) {
            Info(slide, 1, ((char *)slide, LoadFarString(ErrSetVolLabel)));
            return (error & ~MPN_MASK) | MPN_ERR_SKIP;
        }
        /* success:  skip the "extraction" quietly */
        return (error & ~MPN_MASK) | MPN_INF_SKIP;
    }

    return error;

} /* end function mapname() */





/****************************/
/* Function maskDOSdevice() */
/****************************/

static void maskDOSdevice(__G__ pathcomp, last_dot)
    __GDEF
    char *pathcomp, *last_dot;
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
    if (stat(pathcomp, &G.statbuf) == 0) {
        Trace((stderr,
               "maskDOSdevice() stat(\"%s\", buf) st_mode result: %X, %o\n",
               FnFilter1(pathcomp), G.statbuf.st_mode, G.statbuf.st_mode));
    } else {
        Trace((stderr, "maskDOSdevice() stat(\"%s\", buf) failed\n",
               FnFilter1(pathcomp)));
    }
#endif
    if (stat(pathcomp, &G.statbuf) == 0 && S_ISCHR(G.statbuf.st_mode)) {
        extent i;

        /* pathcomp contains a name of a DOS character device (builtin or
         * installed device driver).
         * Prepend a '_' to allow creation of the item in the file system.
         */
        for (i = strlen(pathcomp) + 1; i > 0; --i)
            pathcomp[i] = pathcomp[i - 1];
        pathcomp[0] = '_';
        if (last_dot != (char *)NULL)
            last_dot++;
    }
} /* end function maskDOSdevice() */





#ifdef MAYBE_PLAIN_FAT

/**********************/
/* Function map2fat() */
/**********************/

static void map2fat(pathcomp, last_dot)
    char *pathcomp, *last_dot;
{
    char *pEnd = pathcomp + strlen(pathcomp);

/*---------------------------------------------------------------------------
    Case 1:  filename has no dot, so figure out if we should add one.  Note
    that the algorithm does not try to get too fancy:  if there are no dots
    already, the name either gets truncated at 8 characters or the last un-
    derscore is converted to a dot (only if more characters are saved that
    way).  In no case is a dot inserted between existing characters.

              GRR:  have problem if filename is volume label??

  ---------------------------------------------------------------------------*/

    if (last_dot == (char *)NULL) {   /* no dots:  check for underscores... */
        char *plu = strrchr(pathcomp, '_');   /* pointer to last underscore */

        if ((plu != (char *)NULL) &&    /* found underscore: convert to dot? */
            (MIN(plu - pathcomp, 8) + MIN(pEnd - plu - 1, 3) > 8)) {
            last_dot = plu;       /* be lazy:  drop through to next if-block */
        } else if ((pEnd - pathcomp) > 8)
            /* no underscore; or converting underscore to dot would save less
               chars than leaving everything in the basename */
            pathcomp[8] = '\0';     /* truncate at 8 chars */
        /* else whole thing fits into 8 chars or less:  no change */
    }

/*---------------------------------------------------------------------------
    Case 2:  filename has dot in it, so truncate first half at 8 chars (shift
    extension if necessary) and second half at three.
  ---------------------------------------------------------------------------*/

    if (last_dot != (char *)NULL) {     /* one dot is OK: */
        *last_dot = '.';                /* put the last back in */

        if ((last_dot - pathcomp) > 8) {
            char *p, *q;
            int i;

            p = last_dot;
            q = last_dot = pathcomp + 8;
            for (i = 0;  (i < 4) && *p;  ++i) /* too many chars in basename: */
                *q++ = *p++;                  /*  shift extension left and */
            *q = '\0';                        /*  truncate/terminate it */
        } else if ((pEnd - last_dot) > 4)
            last_dot[4] = '\0';               /* too many chars in extension */
        /* else filename is fine as is:  no change */

        if ((last_dot - pathcomp) > 0 && last_dot[-1] == ' ')
            last_dot[-1] = '_';               /* NO blank in front of '.'! */
    }
} /* end function map2fat() */

#endif /* MAYBE_PLAIN_FAT */





/***********************/
/* Function checkdir() */
/***********************/

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
    static int rootlen = 0;   /* length of rootpath */
    static char *rootpath;    /* user's "extract-to" directory */
    static char *buildpath;   /* full path (so far) to extracted file */
    static char *end;         /* pointer to end of buildpath ('\0') */
#ifdef MSC
    int attrs;                /* work around MSC stat() bug */
#endif

#   define FN_MASK   7
#   define FUNCTION  (flag & FN_MASK)



/*---------------------------------------------------------------------------
    APPEND_DIR:  append the path component to the path being built and check
    for its existence.  If doesn't exist and we are creating directories, do
    so for this one; else signal success or error as appropriate.
  ---------------------------------------------------------------------------*/

    if (FUNCTION == APPEND_DIR) {
        int too_long = FALSE;

        Trace((stderr, "appending dir segment [%s]\n", FnFilter1(pathcomp)));
        while ((*end = *pathcomp++) != '\0')
            ++end;

        /* GRR:  could do better check, see if overrunning buffer as we go:
         * check end-buildpath after each append, set warning variable if
         * within 20 of FILNAMSIZ; then if var set, do careful check when
         * appending.  Clear variable when begin new path. */

        if ((end-buildpath) > FILNAMSIZ-3)  /* need '/', one-char name, '\0' */
            too_long = TRUE;                /* check if extracting directory? */
#ifdef MSC /* MSC 6.00 bug:  stat(non-existent-dir) == 0 [exists!] */
        if (_dos_getfileattr(buildpath, &attrs) || stat(buildpath, &G.statbuf))
#else
        if (SSTAT(buildpath, &G.statbuf))   /* path doesn't exist */
#endif
        {
            if (!G.create_dirs) { /* told not to create (freshening) */
                free(buildpath);
                /* path doesn't exist:  nothing to do */
                return MPN_INF_SKIP;
            }
            if (too_long) {
                Info(slide, 1, ((char *)slide, LoadFarString(PathTooLong),
                  FnFilter1(buildpath)));
                free(buildpath);
                /* no room for filenames:  fatal */
                return MPN_ERR_TOOLONG;
            }
            if (MKDIR(buildpath, 0777) == -1) {   /* create the directory */
                Info(slide, 1, ((char *)slide, LoadFarString(CantCreateDir),
                  FnFilter2(buildpath), FnFilter1(G.filename)));
                free(buildpath);
                /* path didn't exist, tried to create, failed */
                return MPN_ERR_SKIP;
            }
            created_dir = TRUE;
        } else if (!S_ISDIR(G.statbuf.st_mode)) {
            Info(slide, 1, ((char *)slide, LoadFarString(DirIsntDirectory),
              FnFilter2(buildpath), FnFilter1(G.filename)));
            free(buildpath);
            /* path existed but wasn't dir */
            return MPN_ERR_SKIP;
        }
        if (too_long) {
            Info(slide, 1, ((char *)slide, LoadFarString(PathTooLong),
              FnFilter1(buildpath)));
            free(buildpath);
            /* no room for filenames:  fatal */
            return MPN_ERR_TOOLONG;
        }
        *end++ = '/';
        *end = '\0';
        Trace((stderr, "buildpath now = [%s]\n", FnFilter1(buildpath)));
        return MPN_OK;

    } /* end if (FUNCTION == APPEND_DIR) */

/*---------------------------------------------------------------------------
    GETPATH:  copy full path to the string pointed at by pathcomp, and free
    buildpath.
  ---------------------------------------------------------------------------*/

    if (FUNCTION == GETPATH) {
        strcpy(pathcomp, buildpath);
        Trace((stderr, "getting and freeing path [%s]\n",
          FnFilter1(pathcomp)));
        free(buildpath);
        buildpath = end = (char *)NULL;
        return MPN_OK;
    }

/*---------------------------------------------------------------------------
    APPEND_NAME:  assume the path component is the filename; append it and
    return without checking for existence.
  ---------------------------------------------------------------------------*/

    if (FUNCTION == APPEND_NAME) {
#ifdef NOVELL_BUG_WORKAROUND
        if (end == buildpath && !G.pInfo->vollabel) {
            /* work-around for Novell's "overwriting executables" bug:
               prepend "./" to name when no path component is specified */
            *end++ = '.';
            *end++ = '/';
        }
#endif /* NOVELL_BUG_WORKAROUND */
        Trace((stderr, "appending filename [%s]\n", FnFilter1(pathcomp)));
        while ((*end = *pathcomp++) != '\0') {
            ++end;
            if ((end-buildpath) >= FILNAMSIZ) {
                *--end = '\0';
                Info(slide, 1, ((char *)slide, LoadFarString(PathTooLongTrunc),
                  FnFilter1(G.filename), FnFilter2(buildpath)));
                return MPN_INF_TRUNC;   /* filename truncated */
            }
        }
        Trace((stderr, "buildpath now = [%s]\n", FnFilter1(buildpath)));
        /* could check for existence here, prompt for new name... */
        return MPN_OK;
    }

/*---------------------------------------------------------------------------
    INIT:  allocate and initialize buffer space for the file currently being
    extracted.  If file was renamed with an absolute path, don't prepend the
    extract-to path.
  ---------------------------------------------------------------------------*/

    if (FUNCTION == INIT) {
        Trace((stderr, "initializing buildpath to "));
        /* allocate space for full filename, root path, and maybe "./" */
        if ((buildpath = (char *)malloc(strlen(G.filename)+rootlen+3)) ==
            (char *)NULL)
            return MPN_NOMEM;
        if (G.pInfo->vollabel) {
/* GRR:  for network drives, do strchr() and return IZ_VOL_LABEL if not [1] */
            if (renamed_fullpath && pathcomp[1] == ':')
                *buildpath = (char)ToLower(*pathcomp);
            else if (!renamed_fullpath && rootlen > 1 && rootpath[1] == ':')
                *buildpath = (char)ToLower(*rootpath);
            else {
                GETDRIVE(nLabelDrive);   /* assumed that a == 1, b == 2, etc. */
                *buildpath = (char)(nLabelDrive - 1 + 'a');
            }
            nLabelDrive = *buildpath - 'a' + 1;        /* save for mapname() */
            if (uO.volflag == 0 || *buildpath < 'a' || /* no label/bogus disk */
               (uO.volflag == 1 && !isfloppy(nLabelDrive))) /* -$:  no fixed */
            {
                free(buildpath);
                return MPN_VOL_LABEL;    /* skipping with message */
            }
            *buildpath = '\0';
            end = buildpath;
        } else if (renamed_fullpath) {   /* pathcomp = valid data */
            end = buildpath;
            while ((*end = *pathcomp++) != '\0')
                ++end;
        } else if (rootlen > 0) {
            strcpy(buildpath, rootpath);
            end = buildpath + rootlen;
        } else {
            *buildpath = '\0';
            end = buildpath;
        }
        Trace((stderr, "[%s]\n", FnFilter1(buildpath)));
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
        if (pathcomp == (char *)NULL) {
            rootlen = 0;
            return MPN_OK;
        }
        if (rootlen > 0)        /* rootpath was already set, nothing to do */
            return MPN_OK;
        if ((rootlen = strlen(pathcomp)) > 0) {
            int had_trailing_pathsep=FALSE, has_drive=FALSE, add_dot=FALSE;
            char *tmproot;

            if ((tmproot = (char *)malloc(rootlen+3)) == (char *)NULL) {
                rootlen = 0;
                return MPN_NOMEM;
            }
            strcpy(tmproot, pathcomp);
            if (isalpha((uch)tmproot[0]) && tmproot[1] == ':')
                has_drive = TRUE;   /* drive designator */
            if (tmproot[rootlen-1] == '/' || tmproot[rootlen-1] == '\\') {
                tmproot[--rootlen] = '\0';
                had_trailing_pathsep = TRUE;
            }
            if (has_drive && (rootlen == 2)) {
                if (!had_trailing_pathsep)   /* i.e., original wasn't "x:/" */
                    add_dot = TRUE;    /* relative path: add '.' before '/' */
            } else if (rootlen > 0) {     /* need not check "x:." and "x:/" */
#ifdef MSC
                /* MSC 6.00 bug:  stat(non-existent-dir) == 0 [exists!] */
                if (_dos_getfileattr(tmproot, &attrs) ||
                    SSTAT(tmproot, &G.statbuf) || !S_ISDIR(G.statbuf.st_mode))
#else
                if (SSTAT(tmproot, &G.statbuf) || !S_ISDIR(G.statbuf.st_mode))
#endif
                {
                    /* path does not exist */
                    if (!G.create_dirs /* || iswild(tmproot) */ ) {
                        free(tmproot);
                        rootlen = 0;
                        /* treat as stored file */
                        return MPN_INF_SKIP;
                    }
/* GRR:  scan for wildcard characters?  OS-dependent...  if find any, return 2:
 * treat as stored file(s) */
                    /* create directory (could add loop here scanning tmproot
                     * to create more than one level, but really necessary?) */
                    if (MKDIR(tmproot, 0777) == -1) {
                        Info(slide, 1, ((char *)slide,
                          LoadFarString(CantCreateExtractDir),
                          FnFilter1(tmproot)));
                        free(tmproot);
                        rootlen = 0;
                        /* path didn't exist, tried to create, failed: */
                        /* file exists, or need 2+ subdir levels */
                        return MPN_ERR_SKIP;
                    }
                }
            }
            if (add_dot)                    /* had just "x:", make "x:." */
                tmproot[rootlen++] = '.';
            tmproot[rootlen++] = '/';
            tmproot[rootlen] = '\0';
            if ((rootpath = (char *)realloc(tmproot, rootlen+1)) == NULL) {
                free(tmproot);
                rootlen = 0;
                return MPN_NOMEM;
            }
            Trace((stderr, "rootpath now = [%s]\n", FnFilter1(rootpath)));
        }
        return MPN_OK;
    }
#endif /* !SFX || SFX_EXDIR */

/*---------------------------------------------------------------------------
    END:  free rootpath, immediately prior to program exit.
  ---------------------------------------------------------------------------*/

    if (FUNCTION == END) {
        Trace((stderr, "freeing rootpath\n"));
        if (rootlen > 0) {
            free(rootpath);
            rootlen = 0;
        }
        return MPN_OK;
    }

    return MPN_INVALID; /* should never reach */

} /* end function checkdir() */






/***********************/
/* Function isfloppy() */
/***********************/

static int isfloppy(nDrive)  /* more precisely, is it removable? */
    int nDrive;
{
    union REGS regs;

    regs.h.ah = 0x44;
    regs.h.al = 0x08;
    regs.h.bl = (uch)nDrive;
#ifdef __EMX__
    _int86(0x21, &regs, &regs);
    if (WREGS(regs,flags) & 1)
#else
    intdos(&regs, &regs);
    if (WREGS(regs,cflag))        /* error:  do default a/b check instead */
#endif
    {
        Trace((stderr,
          "error in DOS function 0x44 (AX = 0x%04x):  guessing instead...\n",
          (unsigned int)(WREGS(regs,ax))));
        return (nDrive == 1 || nDrive == 2)? TRUE : FALSE;
    } else
        return WREGS(regs,ax)? FALSE : TRUE;
}




/**************************/
/* Function z_dos_chmod() */
/**************************/

static int z_dos_chmod(__G__ fname, attributes)
    __GDEF
    ZCONST char *fname;
    int attributes;
{
    char *name;
    unsigned fnamelength;
    int errv;

    /* set file attributes:
       The DOS `chmod' system call requires to mask out the
       directory and volume_label attribute bits.
       And, a trailing '/' has to be removed from the directory name,
       the DOS `chmod' system call does not accept it. */
    fnamelength = strlen(fname);
    if (fnamelength > 1 && fname[fnamelength-1] == '/' &&
        fname[fnamelength-2] != ':' &&
        (name = (char *)malloc(fnamelength)) != (char *)NULL) {
        strncpy(name, fname, fnamelength-1);
        name[fnamelength-1] = '\0';
    } else {
        name = (char *)fname;
        fnamelength = 0;
    }

#if defined(__TURBOC__) || (defined(__DJGPP__) && (__DJGPP__ >= 2))
#   if (defined(__BORLANDC__) && (__BORLANDC__ >= 0x0452))
#     define Chmod  _rtl_chmod
#   else
#     define Chmod  _chmod
#   endif
    errv = (Chmod(name, 1, attributes & (~FSUBDIR & ~FVOLID)) !=
            (attributes & (~FSUBDIR & ~FVOLID)));
#   undef Chmod
#else /* !(__TURBOC__ || (__DJGPP__ && __DJGPP__ >= 2)) */
    errv = (_dos_setfileattr(name, attributes & (~FSUBDIR & ~FVOLID)) != 0);
#endif /* ?(__TURBOC__ || (__DJGPP__ && __DJGPP__ >= 2)) */
    if (errv)
        Info(slide, 1, ((char *)slide, LoadFarString(AttribsMayBeWrong)));

    if (fnamelength > 0)
        free(name);
    return errv;
} /* end function z_dos_chmod() */




#if (!defined(__GO32__) && !defined(__EMX__))

typedef struct dosfcb {
    uch  flag;        /* ff to indicate extended FCB */
    char res[5];      /* reserved */
    uch  vattr;       /* attribute */
    uch  drive;       /* drive (1=A, 2=B, ...) */
    uch  vn[11];      /* file or volume name */
    char dmmy[5];
    uch  nn[11];      /* holds new name if renaming (else reserved) */
    char dmmy2[9];
} dos_fcb;

/**************************/
/* Function volumelabel() */
/**************************/

static int volumelabel(newlabel)
    ZCONST char *newlabel;
{
#ifdef DEBUG
    char *p;
#endif
    int len = strlen(newlabel);
    int fcbseg, dtaseg, fcboff, dtaoff, retv;
    dos_fcb  fcb, dta, far *pfcb=&fcb, far *pdta=&dta;
    struct SREGS sregs;
    union REGS regs;


/*---------------------------------------------------------------------------
    Label the diskette specified by nLabelDrive using FCB calls.  (Old ver-
    sions of MS-DOS and OS/2 DOS boxes can't use DOS function 3Ch to create
    labels.)  Must use far pointers for MSC FP_* macros to work; must pad
    FCB filenames with spaces; and cannot include dot in 8th position.  May
    or may not need to zero out FCBs before using; do so just in case.
  ---------------------------------------------------------------------------*/

#ifdef WATCOMC_386
    int truseg;

    memset(&sregs, 0, sizeof(sregs));
    memset(&regs, 0, sizeof(regs));
    /* PMODE/W does not support extended versions of any dos FCB functions, */
    /* so we have to use brute force, allocating real mode memory for them. */
    regs.w.ax = 0x0100;
    regs.w.bx = (2 * sizeof(dos_fcb) + 15) >> 4;   /* size in paragraphs */
    int386(0x31, &regs, &regs);            /* DPMI allocate DOS memory */
    if (regs.w.cflag)
        return DF_MDY;                     /* no memory, return default */
    truseg = regs.w.dx;                    /* protected mode selector */
    dtaseg = regs.w.ax;                    /* real mode paragraph */
    fcboff = 0;
    dtaoff = sizeof(dos_fcb);
#ifdef XXX__MK_FP_IS_BROKEN
    /* XXX  This code may not be trustworthy in general, though it is   */
    /* valid with DOS/4GW and PMODE/w, which is all we support for now. */
    regs.w.ax = 6;
    regs.w.bx = truseg;
    int386(0x31, &regs, &regs);            /* convert seg to linear address */
    pfcb = (dos_fcb far *) (((ulg) regs.w.cx << 16) | regs.w.dx);
    /* pfcb = (dos_fcb far *) ((ulg) dtaseg << 4); */
    pdta = pfcb + 1;
#else
    pfcb = MK_FP(truseg, fcboff);
    pdta = MK_FP(truseg, dtaoff);
#endif
    _fmemset((char far *)pfcb, 0, 2 * sizeof(dos_fcb));
    /* we pass the REAL MODE paragraph to the dos interrupts: */
    fcbseg = dtaseg;

#else /* !WATCOMC_386 */

    memset((char *)&dta, 0, sizeof(dos_fcb));
    memset((char *)&fcb, 0, sizeof(dos_fcb));
    fcbseg = FP_SEG(pfcb);
    fcboff = FP_OFF(pfcb);
    dtaseg = FP_SEG(pdta);
    dtaoff = FP_OFF(pdta);
#endif /* ?WATCOMC_386 */

#ifdef DEBUG
    for (p = (char *)&dta; (p - (char *)&dta) < sizeof(dos_fcb); ++p)
        if (*p)
            fprintf(stderr, "error:  dta[%d] = %x\n", (p - (char *)&dta), *p);
    for (p = (char *)&fcb; (p - (char *)&fcb) < sizeof(dos_fcb); ++p)
        if (*p)
            fprintf(stderr, "error:  fcb[%d] = %x\n", (p - (char *)&fcb), *p);
    printf("testing pointer macros:\n");
    segread(&sregs);
    printf("cs = %x, ds = %x, es = %x, ss = %x\n", sregs.cs, sregs.ds, sregs.es,
      sregs.ss);
#endif /* DEBUG */

#if 0
#ifdef __TURBOC__
    bdosptr(0x1a, dta, DO_NOT_CARE);
#else
    (intdosx method below)
#endif
#endif /* 0 */

    /* set the disk transfer address for subsequent FCB calls */
    sregs.ds = dtaseg;
    WREGS(regs,dx) = dtaoff;
    Trace((stderr, "segment:offset of pdta = %x:%x\n", dtaseg, dtaoff));
    Trace((stderr, "&dta = %lx, pdta = %lx\n", (ulg)&dta, (ulg)pdta));
    regs.h.ah = 0x1a;
    F_intdosx(&regs, &regs, &sregs);

    /* fill in the FCB */
    sregs.ds = fcbseg;
    WREGS(regs,dx) = fcboff;
    pfcb->flag = 0xff;          /* extended FCB */
    pfcb->vattr = 0x08;         /* attribute:  disk volume label */
    pfcb->drive = (uch)nLabelDrive;

#ifdef DEBUG
    Trace((stderr, "segment:offset of pfcb = %x:%x\n",
      (unsigned int)(sregs.ds),
      (unsigned int)(WREGS(regs,dx))));
    Trace((stderr, "&fcb = %lx, pfcb = %lx\n", (ulg)&fcb, (ulg)pfcb));
    Trace((stderr, "(2nd check:  labelling drive %c:)\n", pfcb->drive-1+'A'));
    if (pfcb->flag != fcb.flag)
        fprintf(stderr, "error:  pfcb->flag = %d, fcb.flag = %d\n",
          pfcb->flag, fcb.flag);
    if (pfcb->drive != fcb.drive)
        fprintf(stderr, "error:  pfcb->drive = %d, fcb.drive = %d\n",
          pfcb->drive, fcb.drive);
    if (pfcb->vattr != fcb.vattr)
        fprintf(stderr, "error:  pfcb->vattr = %d, fcb.vattr = %d\n",
          pfcb->vattr, fcb.vattr);
#endif /* DEBUG */

    /* check for existing label */
    Trace((stderr, "searching for existing label via FCBs\n"));
    regs.h.ah = 0x11;      /* FCB find first */
#ifdef WATCOMC_386
    _fstrncpy((char far *)&pfcb->vn, "???????????", 11);
#else
    strncpy((char *)fcb.vn, "???????????", 11);   /* i.e., "*.*" */
#endif /* ?WATCOMC_386 */
    Trace((stderr, "fcb.vn = %lx\n", (ulg)fcb.vn));
    Trace((stderr, "regs.h.ah = %x, regs.x.dx = %04x, sregs.ds = %04x\n",
      (unsigned int)(regs.h.ah), (unsigned int)(WREGS(regs,dx)),
      (unsigned int)(sregs.ds)));
    Trace((stderr, "flag = %x, drive = %d, vattr = %x, vn = %s = %s.\n",
      fcb.flag, fcb.drive, fcb.vattr, fcb.vn, pfcb->vn));
    F_intdosx(&regs, &regs, &sregs);

/*---------------------------------------------------------------------------
    If not previously labelled, write a new label.  Otherwise just rename,
    since MS-DOS 2.x has a bug that damages the FAT when the old label is
    deleted.
  ---------------------------------------------------------------------------*/

    if (regs.h.al) {
        Trace((stderr, "no label found\n\n"));
        regs.h.ah = 0x16;                 /* FCB create file */
#ifdef WATCOMC_386
        _fstrncpy((char far *)pfcb->vn, newlabel, len);
        if (len < 11)
            _fstrncpy((char far *)(pfcb->vn+len), "           ", 11-len);
#else
        strncpy((char *)fcb.vn, newlabel, len);
        if (len < 11)   /* fill with spaces */
            strncpy((char *)(fcb.vn+len), "           ", 11-len);
#endif
        Trace((stderr, "fcb.vn = %lx  pfcb->vn = %lx\n", (ulg)fcb.vn,
          (ulg)pfcb->vn));
        Trace((stderr, "flag = %x, drive = %d, vattr = %x\n", fcb.flag,
          fcb.drive, fcb.vattr));
        Trace((stderr, "vn = %s = %s.\n", fcb.vn, pfcb->vn));
        F_intdosx(&regs, &regs, &sregs);
        regs.h.ah = 0x10;                 /* FCB close file */
        if (regs.h.al) {
            Trace((stderr, "unable to write volume name (AL = %x)\n",
              (unsigned int)(regs.h.al)));
            F_intdosx(&regs, &regs, &sregs);
            retv = 1;
        } else {
            F_intdosx(&regs, &regs, &sregs);
            Trace((stderr, "new volume label [%s] written\n", newlabel));
            retv = 0;
        }
    } else {
        Trace((stderr, "found old label [%s]\n\n", dta.vn));  /* not term. */
        regs.h.ah = 0x17;                 /* FCB rename */
#ifdef WATCOMC_386
        _fstrncpy((char far *)pfcb->vn, (char far *)pdta->vn, 11);
        _fstrncpy((char far *)pfcb->nn, newlabel, len);
        if (len < 11)
            _fstrncpy((char far *)(pfcb->nn+len), "           ", 11-len);
#else
        strncpy((char *)fcb.vn, (char *)dta.vn, 11);
        strncpy((char *)fcb.nn, newlabel, len);
        if (len < 11)                     /* fill with spaces */
            strncpy((char *)(fcb.nn+len), "           ", 11-len);
#endif
        Trace((stderr, "fcb.vn = %lx  pfcb->vn = %lx\n", (ulg)fcb.vn,
          (ulg)pfcb->vn));
        Trace((stderr, "fcb.nn = %lx  pfcb->nn = %lx\n", (ulg)fcb.nn,
          (ulg)pfcb->nn));
        Trace((stderr, "flag = %x, drive = %d, vattr = %x\n", fcb.flag,
          fcb.drive, fcb.vattr));
        Trace((stderr, "vn = %s = %s.\n", fcb.vn, pfcb->vn));
        Trace((stderr, "nn = %s = %s.\n", fcb.nn, pfcb->nn));
        F_intdosx(&regs, &regs, &sregs);
        if (regs.h.al) {
            Trace((stderr, "Unable to change volume name (AL = %x)\n",
              (unsigned int)(regs.h.al)));
            retv = 1;
        } else {
            Trace((stderr, "volume label changed to [%s]\n", newlabel));
            retv = 0;
        }
    }
#ifdef WATCOMC_386
    regs.w.ax = 0x0101;                    /* free dos memory */
    regs.w.dx = truseg;
    int386(0x31, &regs, &regs);
#endif
    return retv;

} /* end function volumelabel() */

#endif /* !__GO32__ && !__EMX__ */





#if (defined(USE_EF_UT_TIME) || defined(TIMESTAMP))
/* The following DOS date/time structure is machine-dependent as it
 * assumes "little-endian" byte order.  For MSDOS-specific code, which
 * is run on ix86 CPUs (or emulators), this assumption is valid; but
 * care should be taken when using this code as template for other ports.
 */
typedef union {
    ulg z_dostime;
# ifdef __TURBOC__
    struct ftime ft;            /* system file time record */
# endif
    struct {                    /* date and time words */
        ush ztime;              /* DOS file modification time word */
        ush zdate;              /* DOS file modification date word */
    } zft;
    struct {                    /* DOS date/time components bitfield */
        unsigned zt_se : 5;
        unsigned zt_mi : 6;
        unsigned zt_hr : 5;
        unsigned zd_dy : 5;
        unsigned zd_mo : 4;
        unsigned zd_yr : 7;
    } z_dtf;
} dos_fdatetime;
#endif /* USE_EF_UT_TIME || TIMESTAMP */


/****************************/
/* Function close_outfile() */
/****************************/

void close_outfile(__G)
    __GDEF
 /*
  * MS-DOS VERSION
  *
  * Set the output file date/time stamp according to information from the
  * zipfile directory record for this member, then close the file and set
  * its permissions (archive, hidden, read-only, system).  Aside from closing
  * the file, this routine is optional (but most compilers support it).
  */
{
    /* skip restoring time stamps on user's request */
    if (uO.D_flag <= 1) {
#ifdef USE_EF_UT_TIME
        dos_fdatetime dos_dt;
        iztimes z_utime;
        struct tm *t;
#endif /* USE_EF_UT_TIME */


/*---------------------------------------------------------------------------
        Copy and/or convert time and date variables, if necessary; then set
        the file time/date.  WEIRD BORLAND "BUG":  if output is buffered,
        and if run under at least some versions of DOS (e.g., 6.0), and if
        files are smaller than DOS physical block size (i.e., 512 bytes) (?),
        then files MAY NOT get timestamped correctly--apparently setftime()
        occurs before any data are written to the file, and when file is
        closed and buffers are flushed, timestamp is overwritten with
        current time.  Even with a 32K buffer, this does not seem to occur
        with larger files.  UnZip output is now unbuffered, but if it were
        not, could still avoid problem by adding "fflush(outfile)" just
        before setftime() call.  Weird, huh?
  ---------------------------------------------------------------------------*/

#ifdef USE_EF_UT_TIME
        if (G.extra_field &&
#ifdef IZ_CHECK_TZ
            G.tz_is_valid &&
#endif
            (ef_scan_for_izux(G.extra_field, G.lrec.extra_field_length, 0,
                              G.lrec.last_mod_dos_datetime, &z_utime, NULL)
             & EB_UT_FL_MTIME))
        {
            TTrace((stderr, "close_outfile:  Unix e.f. modif. time = %ld\n",
              z_utime.mtime));
            /* round up (down if "up" overflows) to even seconds */
            if (z_utime.mtime & 1)
                z_utime.mtime = (z_utime.mtime + 1 > z_utime.mtime) ?
                                 z_utime.mtime + 1 : z_utime.mtime - 1;
            TIMET_TO_NATIVE(z_utime.mtime) /* NOP unless MSC 7 or Macintosh */
            t = localtime(&(z_utime.mtime));
        } else
            t = (struct tm *)NULL;
        if (t != (struct tm *)NULL) {
            if (t->tm_year < 80) {
                dos_dt.z_dtf.zt_se = 0;
                dos_dt.z_dtf.zt_mi = 0;
                dos_dt.z_dtf.zt_hr = 0;
                dos_dt.z_dtf.zd_dy = 1;
                dos_dt.z_dtf.zd_mo = 1;
                dos_dt.z_dtf.zd_yr = 0;
            } else {
                dos_dt.z_dtf.zt_se = t->tm_sec >> 1;
                dos_dt.z_dtf.zt_mi = t->tm_min;
                dos_dt.z_dtf.zt_hr = t->tm_hour;
                dos_dt.z_dtf.zd_dy = t->tm_mday;
                dos_dt.z_dtf.zd_mo = t->tm_mon + 1;
                dos_dt.z_dtf.zd_yr = t->tm_year - 80;
            }
        } else {
            dos_dt.z_dostime = G.lrec.last_mod_dos_datetime;
        }
# ifdef __TURBOC__
        setftime(fileno(G.outfile), &dos_dt.ft);
# else
        _dos_setftime(fileno(G.outfile), dos_dt.zft.zdate, dos_dt.zft.ztime);
# endif
#else /* !USE_EF_UT_TIME */
# ifdef __TURBOC__
        setftime(fileno(G.outfile),
                 (struct ftime *)(&(G.lrec.last_mod_dos_datetime)));
# else
        _dos_setftime(fileno(G.outfile),
                      (ush)(G.lrec.last_mod_dos_datetime >> 16),
                      (ush)(G.lrec.last_mod_dos_datetime));
# endif
#endif /* ?USE_EF_UT_TIME */
    }

/*---------------------------------------------------------------------------
    And finally we can close the file...at least everybody agrees on how to
    do *this*.  I think...  Also change the mode according to the stored file
    attributes, since we didn't do that when we opened the dude.
  ---------------------------------------------------------------------------*/

    fclose(G.outfile);

    z_dos_chmod(__G__ G.filename, G.pInfo->file_attr);

} /* end function close_outfile() */





#ifdef TIMESTAMP

/*************************/
/* Function stamp_file() */
/*************************/

int stamp_file(fname, modtime)
    ZCONST char *fname;
    time_t modtime;
{
    dos_fdatetime dos_dt;
    time_t t_even;
    struct tm *t;
    int fd;                             /* file handle */

    /* round up (down if "up" overflows) to even seconds */
    t_even = ((modtime + 1 > modtime) ? modtime + 1 : modtime) & (~1);
    TIMET_TO_NATIVE(t_even)             /* NOP unless MSC 7.0 or Macintosh */
    t = localtime(&t_even);
    if (t == (struct tm *)NULL)
        return -1;                      /* time conversion error */
    if (t->tm_year < 80) {
        dos_dt.z_dtf.zt_se = 0;
        dos_dt.z_dtf.zt_mi = 0;
        dos_dt.z_dtf.zt_hr = 0;
        dos_dt.z_dtf.zd_dy = 1;
        dos_dt.z_dtf.zd_mo = 1;
        dos_dt.z_dtf.zd_yr = 0;
    } else {
        dos_dt.z_dtf.zt_se = t->tm_sec >> 1;
        dos_dt.z_dtf.zt_mi = t->tm_min;
        dos_dt.z_dtf.zt_hr = t->tm_hour;
        dos_dt.z_dtf.zd_dy = t->tm_mday;
        dos_dt.z_dtf.zd_mo = t->tm_mon + 1;
        dos_dt.z_dtf.zd_yr = t->tm_year - 80;
    }
    if (((fd = open((char *)fname, 0)) == -1) ||
# ifdef __TURBOC__
        (setftime(fd, &dos_dt.ft)))
# else
        (_dos_setftime(fd, dos_dt.zft.zdate, dos_dt.zft.ztime)))
# endif
    {
        if (fd != -1)
            close(fd);
        return -1;
    }
    close(fd);
    return 0;

} /* end function stamp_file() */

#endif /* TIMESTAMP */




void prepare_ISO_OEM_translat(__G)
   __GDEF
{
    switch (getdoscodepage()) {
    case 437:
    case 850:
    case 858:
#ifdef IZ_ISO2OEM_ARRAY
        iso2oem = iso2oem_850;
#endif
#ifdef IZ_OEM2ISO_ARRAY
        oem2iso = oem2iso_850;
#endif

    case 932:   /* Japanese */
    case 949:   /* Korean */
    case 936:   /* Chinese, simple */
    case 950:   /* Chinese, traditional */
    case 874:   /* Thai */
    case 1258:  /* Vietnamese */
#ifdef IZ_ISO2OEM_ARRAY
        iso2oem = NULL;
#endif
#ifdef IZ_OEM2ISO_ARRAY
        oem2iso = NULL;
#endif

    default:
#ifdef IZ_ISO2OEM_ARRAY
       iso2oem = NULL;
#endif
#ifdef IZ_OEM2ISO_ARRAY
       oem2iso = NULL;
#endif
    }
} /* end function prepare_ISO_OEM_translat() */




#ifndef SFX

/*************************/
/* Function dateformat() */
/*************************/

int dateformat()
{

/*---------------------------------------------------------------------------
    For those operating systems that support it, this function returns a
    value that tells how national convention says that numeric dates are
    displayed.  Return values are DF_YMD, DF_DMY and DF_MDY (the meanings
    should be fairly obvious).
  ---------------------------------------------------------------------------*/

#ifndef WINDLL
    ush CountryInfo[18];
#if (!defined(__GO32__) && !defined(__EMX__))
    ush far *_CountryInfo = CountryInfo;
    struct SREGS sregs;
    union REGS regs;
#ifdef WATCOMC_386
    ush seg, para;

    memset(&sregs, 0, sizeof(sregs));
    memset(&regs, 0, sizeof(regs));
    /* PMODE/W does not support an extended version of dos function 38,   */
    /* so we have to use brute force, allocating real mode memory for it. */
    regs.w.ax = 0x0100;
    regs.w.bx = 3;                         /* 36 bytes rounds up to 48 */
    int386(0x31, &regs, &regs);            /* DPMI allocate DOS memory */
    if (regs.w.cflag)
        return DF_MDY;                     /* no memory, return default */
    seg = regs.w.dx;
    para = regs.w.ax;

#ifdef XXX__MK_FP_IS_BROKEN
    /* XXX  This code may not be trustworthy in general, though it is
     * valid with DOS/4GW and PMODE/w, which is all we support for now. */
 /* _CountryInfo = (ush far *) (para << 4); */ /* works for some extenders */
    regs.w.ax = 6;
    regs.w.bx = seg;
    int386(0x31, &regs, &regs);            /* convert seg to linear address */
    _CountryInfo = (ush far *) (((ulg) regs.w.cx << 16) | regs.w.dx);
#else
    _CountryInfo = (ush far *) MK_FP(seg, 0);
#endif

    sregs.ds = para;                       /* real mode paragraph */
    regs.w.dx = 0;                         /* no offset from segment */
    regs.w.ax = 0x3800;
    int86x_realmode(0x21, &regs, &regs, &sregs);
    CountryInfo[0] = regs.w.cflag ? 0 : _CountryInfo[0];
    regs.w.ax = 0x0101;
    regs.w.dx = seg;
    int386(0x31, &regs, &regs);              /* DPMI free DOS memory */

#else /* !WATCOMC_386 */

    sregs.ds  = FP_SEG(_CountryInfo);
    regs.x.dx = FP_OFF(_CountryInfo);
    regs.x.ax = 0x3800;
    intdosx(&regs, &regs, &sregs);
#endif /* ?WATCOMC_386 */

#else /* __GO32__ || __EMX__ */
    _dos_getcountryinfo(CountryInfo);
#endif /* ?(__GO32__ || __EMX__) */

    switch(CountryInfo[0]) {
        case 0:
            return DF_MDY;
        case 1:
            return DF_DMY;
        case 2:
            return DF_YMD;
    }
#endif /* !WINDLL */

    return DF_MDY;   /* default for systems without locale info */

} /* end function dateformat() */




#ifndef WINDLL

/**************************************/
/*  Function is_running_on_windows()  */
/**************************************/

static int is_running_on_windows(void)
{
    char *var = getenv("OS");

    /* if the OS env.var says 'Windows_NT' then */
    /* we're likely running on a variant of WinNT */
    if ((var != NULL) && (strcmp("Windows_NT", var) == 0))
        return TRUE;

    /* if the windir env.var is non-null then */
    /* we're likely running on a variant of Win9x */
    /* DOS mode of Win9x doesn't define windir, only winbootdir */
    /* NT's command.com can't see lowercase env. vars */
    var = getenv("windir");
    if ((var != NULL) && (var[0] != '\0'))
        return TRUE;

    return FALSE;
}


/**********************************/
/*  Function check_for_windows()  */
/**********************************/

void check_for_windows(ZCONST char *app)
{
#ifdef SMALL_MEM
    char msg_str[160];          /* enough space for two 79-char-lines  */

    (void)zfstrcpy(msg_buf, WarnUsedOnWindows)
#else
#   define msg_str WarnUsedOnWindows
#endif
    /* Print a warning for users running under Windows */
    /* to reduce bug reports due to running DOS version */
    /* under Windows, when Windows version usually works correctly */
    if (is_running_on_windows())
        printf(msg_str, app);
} /* end function check_for_windows() */


/************************/
/*  Function version()  */
/************************/

void version(__G)
    __GDEF
{
    int len;
#if defined(__DJGPP__) || defined(__WATCOMC__) || \
    (defined(_MSC_VER) && (_MSC_VER != 800))
    char buf[80];
#endif

    len = sprintf((char *)slide, LoadFarString(CompiledWith),

#if defined(__GNUC__)
#  if defined(__DJGPP__)
      (sprintf(buf, "djgpp v%d.%02d / gcc ", __DJGPP__, __DJGPP_MINOR__), buf),
#  elif defined(__GO32__)         /* __GO32__ is defined as "1" only (sigh) */
      "djgpp v1.x / gcc ",
#  elif defined(__EMX__)          /* ...so is __EMX__ (double sigh) */
      "emx+gcc ",
#  else
      "gcc ",
#  endif
      __VERSION__,
#elif defined(__WATCOMC__)
#  if (__WATCOMC__ % 10 != 0)
      "Watcom C/C++", (sprintf(buf, " %d.%02d", __WATCOMC__ / 100,
                               __WATCOMC__ % 100), buf),
#  else
      "Watcom C/C++", (sprintf(buf, " %d.%d", __WATCOMC__ / 100,
                               (__WATCOMC__ % 100) / 10), buf),
#  endif
#elif defined(__TURBOC__)
#  ifdef __BORLANDC__
      "Borland C++",
#    if (__BORLANDC__ < 0x0200)
        " 1.0",
#    elif (__BORLANDC__ == 0x0200)   /* James:  __TURBOC__ = 0x0297 */
        " 2.0",
#    elif (__BORLANDC__ == 0x0400)
        " 3.0",
#    elif (__BORLANDC__ == 0x0410)   /* __BCPLUSPLUS__ = 0x0310 */
        " 3.1",
#    elif (__BORLANDC__ == 0x0452)   /* __BCPLUSPLUS__ = 0x0320 */
        " 4.0 or 4.02",
#    elif (__BORLANDC__ == 0x0460)   /* __BCPLUSPLUS__ = 0x0340 */
        " 4.5",
#    elif (__BORLANDC__ == 0x0500)
        " 5.0",
#    else
        " later than 5.0",
#    endif
#  else
      "Turbo C",
#    if (__TURBOC__ > 0x0401)        /* Kevin:  3.0 -> 0x0401 */
        "++ later than 3.0",
#    elif (__TURBOC__ >= 0x0400)
        "++ 3.0",
#    elif (__TURBOC__ >= 0x0297)     /* see remark for Borland C++ 2.0 */
        "++ 2.0",
#    elif (__TURBOC__ == 0x0296)     /* [662] checked by SPC */
        "++ 1.01",
#    elif (__TURBOC__ == 0x0295)     /* [661] vfy'd by Kevin */
        "++ 1.0",
#    elif (__TURBOC__ == 0x0201)     /* Brian:  2.01 -> 0x0201 */
        " 2.01",
#    elif ((__TURBOC__ >= 0x018d) && (__TURBOC__ <= 0x0200)) /* James: 0x0200 */
        " 2.0",
#    elif (__TURBOC__ > 0x0100)
        " 1.5",                      /* James:  0x0105? */
#    else
        " 1.0",                      /* James:  0x0100 */
#    endif
#  endif
#elif defined(MSC)
#  if defined(_QC) && !defined(_MSC_VER)
      "MS Quick C ", "2.0 or earlier",      /* _QC is defined as 1 */
#  elif defined(_QC) && (_MSC_VER == 600)
      "MS Quick C ", "2.5 (MSC 6.00)",
#  else
      "Microsoft C ",
#    ifdef _MSC_VER
#      if (_MSC_VER == 800)
        "8.0/8.0c (Visual C++ 1.0/1.5)",
#      else
        (sprintf(buf, "%d.%02d", _MSC_VER/100, _MSC_VER%100), buf),
#      endif
#    else
      "5.1 or earlier",
#    endif
#  endif
#else
      "unknown compiler", "",
#endif /* ?compilers */

      "\nMS-DOS",

#if (defined(__GNUC__) || defined(WATCOMC_386))
      " (32-bit)",
#else
#  if defined(M_I86HM) || defined(__HUGE__)
      " (16-bit, huge)",
#  elif defined(M_I86LM) || defined(__LARGE__)
      " (16-bit, large)",
#  elif defined(M_I86MM) || defined(__MEDIUM__)
      " (16-bit, medium)",
#  elif defined(M_I86CM) || defined(__COMPACT__)
      " (16-bit, compact)",
#  elif defined(M_I86SM) || defined(__SMALL__)
      " (16-bit, small)",
#  elif defined(M_I86TM) || defined(__TINY__)
      " (16-bit, tiny)",
#  else
      " (16-bit)",
#  endif
#endif

#ifdef __DATE__
      " on ", __DATE__
#else
      "", ""
#endif
    );

    (*G.message)((zvoid *)&G, slide, (ulg)len, 0);
                                /* MSC can't handle huge macro expansion */

    /* temporary debugging code for Borland compilers only */
#if (defined(__TURBOC__) && defined(DEBUG))
    Info(slide, 0, ((char *)slide, "\tdebug(__TURBOC__ = 0x%04x = %d)\n",
      __TURBOC__, __TURBOC__));
#ifdef __BORLANDC__
    Info(slide, 0, ((char *)slide, "\tdebug(__BORLANDC__ = 0x%04x)\n",
      __BORLANDC__));
#else
    Info(slide, 0, ((char *)slide, "\tdebug(__BORLANDC__ not defined)\n"));
#endif
#ifdef __TCPLUSPLUS__
    Info(slide, 0, ((char *)slide, "\tdebug(__TCPLUSPLUS__ = 0x%04x)\n",
      __TCPLUSPLUS__));
#else
    Info(slide, 0, ((char *)slide, "\tdebug(__TCPLUSPLUS__ not defined)\n"));
#endif
#ifdef __BCPLUSPLUS__
    Info(slide, 0, ((char *)slide, "\tdebug(__BCPLUSPLUS__ = 0x%04x)\n\n",
      __BCPLUSPLUS__));
#else
    Info(slide, 0, ((char *)slide, "\tdebug(__BCPLUSPLUS__ not defined)\n\n"));
#endif
#endif /* __TURBOC__ && DEBUG */

} /* end function version() */

#endif /* !WINDLL */
#endif /* !SFX */

#endif /* !FUNZIP */





#ifdef MY_ZCALLOC       /* Special zcalloc function for MEMORY16 (MSDOS/OS2) */

#if defined(__TURBOC__) && !defined(OS2)
#include <alloc.h>
/* Turbo C malloc() does not allow dynamic allocation of 64K bytes
 * and farmalloc(64K) returns a pointer with an offset of 8, so we
 * must fix the pointer. Warning: the pointer must be put back to its
 * original form in order to free it, use zcfree().
 */

#define MAX_PTR 2       /* reduced from 10 to save space */
/* 10*64K = 640K */

static int next_ptr = 0;

typedef struct ptr_table_s {
    zvoid far *org_ptr;
    zvoid far *new_ptr;
} ptr_table;

static ptr_table table[MAX_PTR];
/* This table is used to remember the original form of pointers
 * to large buffers (64K). Such pointers are normalized with a zero offset.
 * Since MSDOS is not a preemptive multitasking OS, this table is not
 * protected from concurrent access. This hack doesn't work anyway on
 * a protected system like OS/2. Use Microsoft C instead.
 */

zvoid far *zcalloc(unsigned items, unsigned size)
{
    zvoid far *buf;
    ulg bsize = (ulg)items*size;

    if (bsize < (65536L-16L)) {
        buf = farmalloc(bsize);
        if (*(ush*)&buf != 0) return buf;
    } else {
        buf = farmalloc(bsize + 16L);
    }
    if (buf == NULL || next_ptr >= MAX_PTR) return NULL;
    table[next_ptr].org_ptr = buf;

    /* Normalize the pointer to seg:0 */
    *((ush*)&buf+1) += ((ush)((uch*)buf-NULL) + 15) >> 4;
    *(ush*)&buf = 0;
    table[next_ptr++].new_ptr = buf;
    return buf;
}

zvoid zcfree(zvoid far *ptr)
{
    int n;
    if (*(ush*)&ptr != 0) { /* object < 64K */
        farfree(ptr);
        return;
    }
    /* Find the original pointer */
    for (n = next_ptr - 1; n >= 0; n--) {
        if (ptr != table[n].new_ptr) continue;

        farfree(table[n].org_ptr);
        while (++n < next_ptr) {
            table[n-1] = table[n];
        }
        next_ptr--;
        return;
    }
    Trace((stderr, "zcfree: ptr not found!\n"));
}
#endif /* __TURBOC__ */

#if defined(MSC) || defined(__WATCOMC__)
#if (!defined(_MSC_VER) || (_MSC_VER < 700))
#  define _halloc  halloc
#  define _hfree   hfree
#endif

zvoid far *zcalloc(unsigned items, unsigned size)
{
    return (zvoid far *)_halloc((long)items, size);
}

zvoid zcfree(zvoid far *ptr)
{
    _hfree((void huge *)ptr);
}
#endif /* MSC || __WATCOMC__ */

#endif /* MY_ZCALLOC */


#ifndef FUNZIP

#if (defined(__GO32__) || defined(__EMX__))

#if (!defined(__DJGPP__) || (__DJGPP__ < 2) || \
     ((__DJGPP__ == 2) && (__DJGPP_MINOR__ < 2)))
int volatile _doserrno;
#endif /* not "djgpp v2.02 or newer" */

#if (!defined(__DJGPP__) || (__DJGPP__ < 2))

unsigned _dos_getcountryinfo(void *countrybuffer)
{
    asm("movl %0, %%edx": : "g" (countrybuffer));
    asm("movl $0x3800, %eax");
    asm("int $0x21": : : "%eax", "%ebx", "%ecx", "%edx", "%esi", "%edi");
    _doserrno = 0;
    asm("jnc 1f");
    asm("movl %%eax, %0": "=m" (_doserrno));
    asm("1:");
    return (unsigned)_doserrno;
}

unsigned _dos_setftime(int fd, unsigned dosdate, unsigned dostime)
{
    asm("movl %0, %%ebx": : "g" (fd));
    asm("movl %0, %%ecx": : "g" (dostime));
    asm("movl %0, %%edx": : "g" (dosdate));
    asm("movl $0x5701, %eax");
    asm("int $0x21": : : "%eax", "%ebx", "%ecx", "%edx", "%esi", "%edi");
    _doserrno = 0;
    asm("jnc 1f");
    asm("movl %%eax, %0": "=m" (_doserrno));
    errno = EBADF;
    asm("1:");
    return (unsigned)_doserrno;
}

unsigned _dos_setfileattr(const char *name, unsigned attr)
{
#if 0   /* stripping of trailing '/' is not needed for unzip-internal use */
    unsigned namlen = strlen(name);
    char *i_name = alloca(namlen + 1);

    strcpy(i_name, name);
    if (namlen > 1 && i_name[namlen-1] == '/' && i_name[namlen-2] != ':')
        i_name[namlen-1] = '\0';
    asm("movl %0, %%edx": : "g" (i_name));
#else
    asm("movl %0, %%edx": : "g" (name));
#endif
    asm("movl %0, %%ecx": : "g" (attr));
    asm("movl $0x4301, %eax");
    asm("int $0x21": : : "%eax", "%ebx", "%ecx", "%edx", "%esi", "%edi");
    _doserrno = 0;
    asm("jnc 1f");
    asm("movl %%eax, %0": "=m" (_doserrno));
    switch (_doserrno) {
    case 2:
    case 3:
           errno = ENOENT;
           break;
    case 5:
           errno = EACCES;
           break;
    }
    asm("1:");
    return (unsigned)_doserrno;
}

void _dos_getdrive(unsigned *d)
{
    asm("movl $0x1900, %eax");
    asm("int $0x21": : : "%eax", "%ebx", "%ecx", "%edx", "%esi", "%edi");
    asm("xorb %ah, %ah");
    asm("incb %al");
    asm("movl %%eax, %0": "=a" (*d));
}

unsigned _dos_creat(const char *path, unsigned attr, int *fd)
{
    asm("movl $0x3c00, %eax");
    asm("movl %0, %%edx": :"g" (path));
    asm("movl %0, %%ecx": :"g" (attr));
    asm("int $0x21": : : "%eax", "%ebx", "%ecx", "%edx", "%esi", "%edi");
    asm("movl %%eax, %0": "=a" (*fd));
    _doserrno = 0;
    asm("jnc 1f");
    _doserrno = *fd;
    switch (_doserrno) {
    case 3:
           errno = ENOENT;
           break;
    case 4:
           errno = EMFILE;
           break;
    case 5:
           errno = EACCES;
           break;
    }
    asm("1:");
    return (unsigned)_doserrno;
}

unsigned _dos_close(int fd)
{
    asm("movl %0, %%ebx": : "g" (fd));
    asm("movl $0x3e00, %eax");
    asm("int $0x21": : : "%eax", "%ebx", "%ecx", "%edx", "%esi", "%edi");
    _doserrno = 0;
    asm("jnc 1f");
    asm ("movl %%eax, %0": "=m" (_doserrno));
    if (_doserrno == 6) {
          errno = EBADF;
    }
    asm("1:");
    return (unsigned)_doserrno;
}

#endif /* !__DJGPP__ || (__DJGPP__ < 2) */


static int volumelabel(ZCONST char *name)
{
    int fd;

    return _dos_creat(name, FA_LABEL, &fd) ? fd : _dos_close(fd);
}


#if (defined(__DJGPP__) && (__DJGPP__ >= 2))

#include <dpmi.h>               /* These includes for the country info */
#include <go32.h>
#include <sys/farptr.h>

/* The above _dos_getcountryinfo function doesn't work with djgpp v2, presumably
 * because ds is not set correctly (does it really work at all?). Note that
 * this version only sets the date (ie. CountryInfo[0]).
 */
unsigned _dos_getcountryinfo(void *countrybuffer)
{
   __dpmi_regs regs;

   regs.x.ax = 0x3800;
   regs.x.dx = __tb & 0x0f;
   regs.x.ds = (__tb >> 4) & 0xffff;
   _doserrno = __dpmi_int(0x21, &regs);

   *(ush*)countrybuffer = _farpeekw(_dos_ds, __tb & 0xfffff);

   return (unsigned)_doserrno;
}


/* Disable determination of "x" bit in st_mode field for [f]stat() calls. */
int _is_executable (const char *path, int fhandle, const char *ext)
{
    return 0;
}

#ifndef USE_DJGPP_GLOB
/* Prevent globbing of filenames.  This gives the same functionality as
 * "stubedit <program> globbing=no" did with DJGPP v1.
 */
char **__crt0_glob_function(char *_arg)
{
    return NULL;
}
#endif /* !USE_DJGPP_GLOB */

#ifndef USE_DJGPP_ENV
/* Reduce the size of the executable and remove the functionality to read
 * the program's environment from whatever $DJGPP points to.
 */
void __crt0_load_environment_file(char *_app_name)
{
}
#endif /* !USE_DJGPP_ENV */

#endif /* __DJGPP__ >= 2 */
#endif /* __GO32__ || __EMX__ */



static int getdoscodepage(void)
{
    union REGS regs;

    WREGS(regs,ax) = 0x6601;
#ifdef __EMX__
    _int86(0x21, &regs, &regs);
    if (WREGS(regs,flags) & 1)
#else
    intdos(&regs, &regs);
    if (WREGS(regs,cflag))
#endif
    {
        Trace((stderr,
          "error in DOS function 0x66 (AX = 0x%04x): default to 850...\n",
          (unsigned int)(WREGS(regs,ax))));
        return 858;
    } else
        return WREGS(regs,bx);
}



#ifdef __EMX__
#ifdef MORE

/*************************/
/* Function screensize() */
/*************************/

int screensize(int *tt_rows, int *tt_cols)
{
    int scr_dimen[2];           /* scr_dimen[0]: columns, src_dimen[1]: rows */

    _scrsize(scr_dimen);
    if (tt_rows != NULL) *tt_rows = scr_dimen[1];
    if (tt_cols != NULL) *tt_cols = scr_dimen[0];
    return 0;
}

#endif /* MORE */
#endif /* __EMX__ */



#ifdef WATCOMC_386
#ifdef MORE
#include <graph.h>

/*************************/
/* Function screensize() */
/*************************/

int screensize(int *tt_rows, int *tt_cols)
{
    struct videoconfig vc;

    _getvideoconfig(&vc);
    if (tt_rows != NULL) *tt_rows = (int)(vc.numtextrows);
    if (tt_cols != NULL) *tt_cols = (int)(vc.numtextcols);
    return 0;
}

#endif /* MORE */


static struct RMINFO {
    ulg edi, esi, ebp;
    ulg reserved;
    ulg ebx, edx, ecx, eax;
    ush flags;
    ush es,ds,fs,gs;
    ush ip_ignored,cs_ignored;
    ush sp,ss;
};

/* This function is used to call dos interrupts that may not be supported
 * by some particular 32-bit DOS extender.  It uses DPMI function 300h to
 * simulate a real mode call of the interrupt.  The caller is responsible
 * for providing real mode addresses of any buffer areas used.  The docs
 * for PMODE/W imply that this should not be necessary for calling the DOS
 * interrupts that it doesn't extend, but it crashes when this isn't used. */

static int int86x_realmode(int inter_no, union REGS *in,
                           union REGS *out, struct SREGS *seg)
{
    union REGS local;
    struct SREGS localseg;
    struct RMINFO rmi;
    int r;

    rmi.eax = in->x.eax;
    rmi.ebx = in->x.ebx;
    rmi.ecx = in->x.ecx;
    rmi.edx = in->x.edx;
    rmi.edi = in->x.edi;
    rmi.esi = in->x.esi;
    rmi.ebp = rmi.reserved = 0L;
    rmi.es = seg->es;
    rmi.ds = seg->ds;
    rmi.fs = seg->fs;
    rmi.gs = seg->gs;
    rmi.sp = rmi.ss = rmi.ip_ignored = rmi.cs_ignored = rmi.flags = 0;
    memset(&local, 0, sizeof(local));
    memset(&localseg, 0, sizeof(localseg));
    local.w.ax = 0x0300;
    local.h.bl = inter_no;
    local.h.bh = 0;
    local.w.cx = 0;
    localseg.es = FP_SEG(&rmi);
    local.x.edi = FP_OFF(&rmi);
    r = int386x(0x31, &local, &local, &localseg);
    out->x.eax = rmi.eax;
    out->x.ebx = rmi.ebx;
    out->x.ecx = rmi.ecx;
    out->x.edx = rmi.edx;
    out->x.edi = rmi.edi;
    out->x.esi = rmi.esi;
    out->x.cflag = rmi.flags & INTR_CF;
    return r;
}

#endif /* WATCOMC_386 */




#ifdef DOS_STAT_BANDAID

/* This papers over a bug in Watcom 10.6's standard library...sigh.
 * Apparently it applies to both the DOS and Win32 stat()s. */

int stat_bandaid(const char *path, struct stat *buf)
{
    char newname[4];

    if (!stat(path, buf))
        return 0;
    else if (!strcmp(path, ".") || (path[0] && !strcmp(path + 1, ":."))) {
        strcpy(newname, path);
        newname[strlen(path) - 1] = '\\';   /* stat(".") fails for root! */
        return stat(newname, buf);
    } else
        return -1;
}

#endif /* DOS_STAT_BANDAID */

#endif /* !FUNZIP */
