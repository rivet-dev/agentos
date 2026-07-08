/*
  Copyright (c) 1990-2007 Info-ZIP.  All rights reserved.

  See the accompanying file LICENSE, version 2000-Apr-09 or later
  (the contents of which are also included in unzip.h) for terms of use.
  If, for some reason, all these files are missing, the Info-ZIP license
  also may be found at:  ftp://ftp.info-zip.org/pub/infozip/license.html
*/
/*---------------------------------------------------------------------------

  macos.c

  Macintosh-specific routines for use with Info-ZIP's UnZip 5.4 and later.

  Contains:
                    do_wild ()
                    mapattr ()
                    checkdir ()
                    version ()
                    macmkdir ()
                    macopen ()
                    maccreat ()
                    macread ()
                    macwrite ()
                    macclose ()
                    maclseek ()
                    BuildMacFilename()
                    SetFinderInfo ()
                    isMacOSexfield ()
                    makePPClong ()
                    makePPCword ()
                    PrintMacExtraInfo ()
                    GetExtraFieldData ()
                    DecodeMac3ExtraField ()
                    DecodeJLEEextraField ()
                    PrintTextEncoding ()
                    MacGlobalsInit ()

  ---------------------------------------------------------------------------*/


/*****************************************************************************/
/*  Includes                                                                 */
/*****************************************************************************/

#define UNZIP_INTERNAL
#include "unzip.h"

#include <script.h>
#include <sound.h>

#include "pathname.h"
#include "helpers.h"
#include "macstuff.h"
#include "mactime.h"
#include "macbin3.h"

/*****************************************************************************/
/*  Macros, typedefs                                                         */
/*****************************************************************************/

/* disable ZipIt support
#define SwitchZIPITefSupportOff */

#define read_only   file_attr       /* for readability only */
#define EB_MAX_OF_VARDATA   1300    /* max possible datasize of extra-field */

#ifdef WILD_STOP_AT_DIR
#  define WESEP     , (oU.W_flag ? ':' : '\0')
#else
#  define WESEP
#endif


/*****************************************************************************/
/*  Global Vars                                                              */
/*****************************************************************************/

/*  Note: sizeof() returns the size of this allusion
          13 is current length of "XtraStuf.mac:"      */
extern const char ResourceMark[13]; /* var is initialized in file pathname.c */

Boolean MacUnzip_Noisy;            /* MacUnzip_Noisy is also used by console */

MACINFO newExtraField;  /* contains all extra-field data */
short MacZipMode;


/*****************************************************************************/
/*  Module level Vars                                                        */
/*****************************************************************************/

static const char MacPathEnd = ':';   /* the Macintosh dir separator */

static int created_dir;        /* used in mapname(), checkdir() */
static int renamed_fullpath;   /* ditto */
static FSSpec CurrentFile;

static Boolean OpenZipFile = true;
static Boolean UseUT_ExtraField     = false;
static Boolean IgnoreEF_Macfilename = false;
static short fileSystemID;

static uch *attrbuff = NULL;
static uch *malloced_attrbuff = NULL;

const short HFS_fileSystem = 0;

/*****************************************************************************/
/*  Prototypes                                                               */
/*****************************************************************************/

extern char *GetUnZipInfoVersions(void);

static OSErr SetFinderInfo(__GPRO__ int SetTime, FSSpec *spec, MACINFO *mi);
static Boolean GetExtraFieldData(short *MacZipMode, MACINFO *mi);
static uch *scanMacOSexfield(uch *ef_ptr, unsigned ef_len,
                             short *MacZipMode);
static Boolean isMacOSexfield(unsigned id, unsigned size, short *MacZipMode);
static void PrintMacExtraInfo(MACINFO *mi);
static OSErr SetFileTime(__GPRO);
static void DecodeMac3ExtraField(ZCONST uch *buff, MACINFO *mi);
static void DecodeJLEEextraField(ZCONST uch *buff, MACINFO *mi);
static void DecodeZPITextraField(ZCONST uch *buff, MACINFO *mi);
static char *PrintTextEncoding(short script);
static void BuildMacFilename(void);

/*****************************************************************************/
/*  Constants (strings, etc.)                                                */
/*****************************************************************************/

static ZCONST char Far CannotCreateFile[] = "error:  cannot create %s\n";

static ZCONST char Far OutOfMemEF[] = "Can't allocate memory to uncompress"\
                                      " file attributes.\n";

static ZCONST char Far ErrUncmpEF[] = "Error uncompressing file attributes.\n";

static ZCONST char Far No64Time[]   = "Don't support 64 bit Timevalues; get "\
                                      " a newer version of MacZip \n";

static ZCONST char Far NoUniCode[]  = "Don't support Unicoded Filenames; get"\
                                      " a newer version of MacZip\n";

static ZCONST char Far ZitIt_EF[]   = "warning: found ZipIt extra field  "\
                                      " -> file is probably not "\
                                      "usable!!\n";

static ZCONST char Far CantAllocateWildcard[] =
    "warning:  cannot allocate wildcard buffers\n";

static ZCONST char Far ErrNoTimeSet[] = "error (%d): cannot set the time for"\
                                        " %s\n";

static ZCONST char Far MacBinaryMsg[] = "\n   ... decoding MacBinary ";

static ZCONST char Far WarnDirTraversSkip[] =
  "warning:  skipped \"../\" path component(s) in %s\n";

static ZCONST char Far Creating[] = "   creating: %s\n";

static ZCONST char Far ConversionFailed[] =
  "mapname:  conversion of %s failed\n";

static ZCONST char Far PathTooLong[] = "checkdir error:  path too long: %s\n";

static ZCONST char Far CantCreateDir[] = "checkdir error:  cannot create %s\n\
                 unable to process %s.\n";

static ZCONST char Far DirIsntDirectory[] =
  "checkdir error:  %s exists but is not directory\n\
                 unable to process %s.\n";

static ZCONST char Far PathTooLongTrunc[] =
  "checkdir warning:  path too long; truncating\n                   %s\n\
                -> %s\n";

static ZCONST char Far CantCreateExtractDir[] =
  "checkdir:  cannot create extraction directory: %s\n";

static ZCONST char Far FilenameToLong[] =
  "Filename is to long; truncated: %s\n";

/*****************************************************************************/
/*  Functions                                                                */
/*****************************************************************************/

#ifndef SFX

/**********************/
/* Function do_wild() */   /* for porting: dir separator; match(ignore_case) */
/**********************/

char *do_wild(__G__ wildspec)
    __GDEF
    ZCONST char *wildspec;  /* only used first time on a given dir */
{
    static DIR *wild_dir = (DIR *)NULL;
    static ZCONST char *wildname;
    static char *dirname, matchname[FILNAMSIZ];
    static int notfirstcall=FALSE, have_dirname;
    static unsigned long dirnamelen;
    struct dirent *file;

    /* Even when we're just returning wildspec, we *always* do so in
     * matchname[]--calling routine is allowed to append four characters
     * to the returned string, and wildspec may be a pointer to argv[].
     */
    if (!notfirstcall) {    /* first call:  must initialize everything */
        notfirstcall = TRUE;

        /* Folder names must always end with a colon */
        if (uO.exdir[strlen(uO.exdir)-1] != ':') {
            strcat(uO.exdir, ":");
        }

        MacUnzip_Noisy = !uO.qflag;

        if (MacUnzip_Noisy) printf("%s \n\n", GetUnZipInfoVersions());

        /* break the wildspec into a directory part and a wildcard filename */
        if ((wildname = (ZCONST char *)strrchr(wildspec, ':')) ==
            (ZCONST char *)NULL) {
            dirname = ":";
            dirnamelen = 1;
            have_dirname = FALSE;
            wildname = wildspec;
        } else {
            ++wildname;     /* point at character after ':' */
            dirnamelen = wildname - wildspec;
            if ((dirname = (char *)malloc(dirnamelen+1)) == (char *)NULL) {
                Info(slide, 0x201, ((char *)slide,
                  LoadFarString(CantAllocateWildcard)));
                strncpy(matchname, wildspec, FILNAMSIZ);
                matchname[FILNAMSIZ-1] = '\0';
                return matchname;   /* but maybe filespec was not a wildcard */
            }
            strncpy(dirname, wildspec, dirnamelen);
            dirname[dirnamelen] = '\0';   /* terminate for strcpy below */
            have_dirname = TRUE;
        }

        if ((wild_dir = opendir(dirname)) != (DIR *)NULL) {
            while ((file = readdir(wild_dir)) != (struct dirent *)NULL) {
                if (match(file->d_name, wildname, 0 WESEP)) { /* 0=case sens.*/
                    if (have_dirname) {
                        strcpy(matchname, dirname);
                        strcpy(matchname+dirnamelen, file->d_name);
                    } else
                        strcpy(matchname, file->d_name);
                    return matchname;
                }
            }
            /* if we get to here directory is exhausted, so close it */
            closedir(wild_dir);
            wild_dir = (DIR *)NULL;
        }

        /* return the raw wildspec in case that works (e.g., directory not
         * searchable, but filespec was not wild and file is readable) */
        strncpy(matchname, wildspec, FILNAMSIZ);
        matchname[FILNAMSIZ-1] = '\0';
        return matchname;
    }

    /* last time through, might have failed opendir but returned raw wildspec */
    if (wild_dir == (DIR *)NULL) {
        notfirstcall = FALSE; /* nothing left to try--reset for new wildspec */
        if (have_dirname)
            free(dirname);
        return (char *)NULL;
    }

    /* If we've gotten this far, we've read and matched at least one entry
     * successfully (in a previous call), so dirname has been copied into
     * matchname already.
     */
    while ((file = readdir(wild_dir)) != (struct dirent *)NULL)
        if (match(file->d_name, wildname, 0 WESEP)) {   /* 0 == case sens. */
            if (have_dirname) {
                /* strcpy(matchname, dirname); */
                strcpy(matchname+dirnamelen, file->d_name);
            } else
                strcpy(matchname, file->d_name);
            return matchname;
        }

    closedir(wild_dir);     /* have read at least one entry; nothing left */
    wild_dir = (DIR *)NULL;
    notfirstcall = FALSE;   /* reset for new wildspec */
    if (have_dirname)
        free(dirname);
    return (char *)NULL;

} /* end function do_wild() */

#endif /* !SFX */





/***************************/
/* Function open_outfile() */
/***************************/

int open_outfile(__G)         /* return 1 if fail */
    __GDEF
{
    short outfd, fDataFork = true;

#ifdef DLL
    if (G.redirect_data)
        return (redirect_outfile(__G) == FALSE);
#endif
    Trace((stderr, "open_outfile:  trying to open (%s) for writing\n",
      FnFilter1(G.filename)));

    if (!uO.aflag) {
         /* unknown type documents */
         /* all files are considered to be of type 'TEXT' and creator 'hscd' */
         /* this is the default type for CDROM ISO-9660 without Apple extensions */
        newExtraField.fpb.hFileInfo.ioFlFndrInfo.fdType    =  'TEXT';
        newExtraField.fpb.hFileInfo.ioFlFndrInfo.fdCreator =  'hscd';
    } else {
     /* unknown text-files defaults to 'TEXT' */
        newExtraField.fpb.hFileInfo.ioFlFndrInfo.fdType    =  'TEXT';
     /* Bare Bones BBEdit */
        newExtraField.fpb.hFileInfo.ioFlFndrInfo.fdCreator =  'R*ch';
    }

    BuildMacFilename();

    if  (MacZipMode <= TomBrownZipIt2_EF)
        {
        fDataFork = true;
        }
    else
        {
        fDataFork = (newExtraField.flags & EB_M3_FL_DATFRK) ? TRUE : FALSE;
        }


    if ((outfd = maccreat(G.filename)) != -1) {
        outfd = macopen(G.filename, (fDataFork) ? 1 : 2);
    }

    if (outfd == -1) {
        G.outfile = (FILE *)NULL;
        Info(slide, 0x401, ((char *)slide, LoadFarString(CannotCreateFile),
          FnFilter1(G.filename)));
        return 1;
    }
    G.outfile = (FILE *)outfd;
    Trace((stderr, "open_outfile:  successfully opened (%s) for writing\n",
      FnFilter1(G.filename)));

    return 0;

} /* end function open_outfile() */





/**********************/
/* Function mapattr() */
/**********************/

int mapattr(__G)
    __GDEF
{
    /* only care about read-only bit, so just look at MS-DOS side of attrs */
    G.pInfo->read_only = (unsigned)(G.crec.external_file_attributes & 1);
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
    int killed_ddot = FALSE;       /* is set when skipping "../" pathcomp */
    int error = MPN_OK;
    register unsigned workch;      /* hold the character being tested */


/*---------------------------------------------------------------------------
    Initialize various pointers and counters and stuff.
  ---------------------------------------------------------------------------*/

    if (G.pInfo->vollabel)
        return MPN_VOL_LABEL;   /* can't set disk volume labels on Macintosh */

    /* can create path as long as not just freshening, or if user told us */
    G.create_dirs = (!uO.fflag || renamed);

    created_dir = FALSE;        /* not yet */

    /* user gave full pathname:  don't prepend rootpath */
    renamed_fullpath = (renamed && (*G.filename == '/'));

    if (checkdir(__G__ (char *)NULL, INIT) == MPN_NOMEM)
        return MPN_NOMEM;       /* initialize path buffer, unless no memory */

    *pathcomp = '\0';           /* initialize translation buffer */
    pp = pathcomp;              /* point to translation buffer */

    if (uO.jflag)               /* junking directories */
        cp = (char *)strrchr(G.filename, '/');
    if (cp == (char *)NULL) {   /* no '/' or not junking dirs */
        cp = G.filename;        /* point to internal zipfile-member pathname */
        if (renamed_fullpath)
            ++cp;               /* skip over leading '/' */
    } else
        ++cp;                   /* point to start of last component of path */

/*---------------------------------------------------------------------------
    Begin main loop through characters in filename.
  ---------------------------------------------------------------------------*/

    while ((workch = (uch)*cp++) != 0) {

        switch (workch) {
            case '/':             /* can assume -j flag not given */
                *pp = '\0';
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

            case ':':
                *pp++ = '/';      /* ':' is a pathseperator for HFS  */
                break;

            case ';':             /* VMS version (or DEC-20 attrib?) */
                lastsemi = pp;         /* keep for now; remove VMS ";##" */
                *pp++ = (char)workch;  /*  later, if requested */
                break;

            default:
                /* allow European characters in filenames: */
                if (isprint(workch) || (128 <= workch && workch <= 254))
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
            /* set dir time (note trailing '/') */
            return (error & ~MPN_MASK) | MPN_CREATED_DIR;
        }
        /* dir existed already; don't look for data to extract */
        return (error & ~MPN_MASK) | MPN_INF_SKIP;
    }

    *pp = '\0';                   /* done with pathcomp:  terminate it */

    /* if not saving them, remove VMS version numbers (appended ";###") */
    if (!uO.V_flag && lastsemi) {
        pp = lastsemi + 1;
        while (isdigit((uch)(*pp)))
            ++pp;
        if (*pp == '\0')          /* only digits between ';' and end:  nuke */
            *lastsemi = '\0';
    }

    if (*pathcomp == '\0') {
        Info(slide, 1, ((char *)slide, LoadFarString(ConversionFailed),
          FnFilter1(G.filename)));
        return (error & ~MPN_MASK) | MPN_ERR_SKIP;
    }

    checkdir(__G__ pathcomp, APPEND_NAME);  /* returns 1 if truncated: care? */
    checkdir(__G__ G.filename, GETPATH);

    return error;

} /* end function mapname() */





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

#   define FN_MASK   7
#   define FUNCTION  (flag & FN_MASK)


/*---------------------------------------------------------------------------
    APPEND_DIR:  append the path component to the path being built and check
    for its existence.  If doesn't exist and we are creating directories, do
    so for this one; else signal success or error as appropriate.
  ---------------------------------------------------------------------------*/

    if (FUNCTION == APPEND_DIR) {
        int too_long = FALSE;
#ifdef SHORT_NAMES
        char *old_end = end;
#endif

        Trace((stderr, "appending dir segment [%s]\n", FnFilter1(pathcomp)));
        while ((*end = *pathcomp++) != '\0')
            ++end;
#ifdef SHORT_NAMES   /* path components restricted to 14 chars, typically */
        if ((end-old_end) > NAME_MAX)
            *(end = old_end + NAME_MAX) = '\0';
#endif

        /* GRR:  could do better check, see if overrunning buffer as we go:
         * check end-buildpath after each append, set warning variable if
         * within 20 of FILNAMSIZ; then if var set, do careful check when
         * appending.  Clear variable when begin new path. */

        if ((end-buildpath) > NAME_MAX-3)   /* need ':', one-char name, '\0' */
            too_long = TRUE;                /* check if extracting directory? */
        if (stat(buildpath, &G.statbuf)) {  /* path doesn't exist */
            if (!G.create_dirs) { /* told not to create (freshening) */
                free(buildpath);
                return MPN_INF_SKIP;    /* path doesn't exist: nothing to do */
            }
            if (too_long) {
                Info(slide, 1, ((char *)slide, LoadFarString(PathTooLong),
                  FnFilter1(buildpath)));
                free(buildpath);
                /* no room for filenames:  fatal */
                return MPN_ERR_TOOLONG;
            }
            if (macmkdir(buildpath) == -1) {   /* create the directory */
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
        *end++ = ':';
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
#ifdef SHORT_NAMES
        char *old_end = end;
#endif

        Trace((stderr, "appending filename [%s]\n", FnFilter1(pathcomp)));
        while ((*end = *pathcomp++) != '\0') {
            ++end;
#ifdef SHORT_NAMES  /* truncate name at 14 characters, typically */
            if ((end-old_end) > NAME_MAX)
                *(end = old_end + NAME_MAX) = '\0';
#endif
            if ((end-buildpath) >= NAME_MAX) {
                *--end = '\0';
                Info(slide, 0x201, ((char *)slide,
                  LoadFarString(PathTooLongTrunc),
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
        if ((buildpath = (char *)malloc(strlen(G.filename)+rootlen+2))
            == (char *)NULL)
            return MPN_NOMEM;
        if ((rootlen > 0) && !renamed_fullpath) {
            strcpy(buildpath, rootpath);
            end = buildpath + rootlen;
        } else {
            end = buildpath;
            if (!renamed_fullpath && !uO.jflag) {
                *end++ = ':';           /* indicate relative path */
            }
            *end = '\0';
        }
        Trace((stderr, "[%s]\n", FnFilter1(buildpath)));
        return MPN_OK;
    }

/*---------------------------------------------------------------------------
    ROOT:  if appropriate, store the path in rootpath and create it if
    necessary; else assume it's a zipfile member and return.  This path
    segment gets used in extracting all members from every zipfile specified
    on the command line.
  ---------------------------------------------------------------------------*/

#if (!defined(SFX) || defined(SFX_EXDIR))
    if (FUNCTION == ROOT) {
        Trace((stderr, "initializing root path to [%s]\n",
          FnFilter1pathcomp)));
        if (pathcomp == (char *)NULL) {
            rootlen = 0;
            return MPN_OK;
        }
        if (rootlen > 0)        /* rootpath was already set, nothing to do */
            return MPN_OK;
        if ((rootlen = strlen(pathcomp)) > 0) {
            char *tmproot;

            if ((tmproot = (char *)malloc(rootlen+2)) == (char *)NULL) {
                rootlen = 0;
                return MPN_NOMEM;
            }
            strcpy(tmproot, pathcomp);
            if (tmproot[rootlen-1] == ':') {
                tmproot[--rootlen] = '\0';     /* strip trailing delimiter */
            }
            if (rootlen > 0 && (stat(tmproot, &G.statbuf) ||
                !S_ISDIR(G.statbuf.st_mode)))
            {   /* path does not exist */
                if (!G.create_dirs /* || iswild(tmproot) */ ) {
                    free(tmproot);
                    rootlen = 0;
                    /* skip (or treat as stored file) */
                    return MPN_INF_SKIP;
                }
                /* create the directory (could add loop here scanning tmproot
                 * to create more than one level, but why really necessary?) */
                if (macmkdir(tmproot) == -1) {
                    Info(slide, 1, ((char *)slide,
                      LoadFarString(CantCreateExtractDir),
                      FnFilter1(tmproot)));
                    free(tmproot);
                    rootlen = 0;
                    /* path didn't exist, tried to create, and failed: */
                    /* file exists, or 2+ subdir levels required */
                    return MPN_ERR_SKIP;
                }
            }
            tmproot[rootlen++] = ':';
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





/****************************/
/* Function close_outfile() */
/****************************/

void close_outfile(__G)
    __GDEF
{
    OSErr err;

    if (fileno(G.outfile) == 1)
        return;         /* don't attempt to close or set time on stdout */

    err = (OSErr)fclose(G.outfile);

    /* finally set FinderInfo */
    if  (MacZipMode >= JohnnyLee_EF)
        {
        /* skip restoring time stamps on user's request */
        err = SetFinderInfo(__G__ (uO.D_flag <= 1),
                            &CurrentFile, &newExtraField);
        printerr("close_outfile SetFinderInfo ", err, err,
                 __LINE__, __FILE__, G.filename);
        }
    else  /* unknown extra field, set at least file time/dates */
        {
        /* skip restoring time stamps on user's request */
        if (uO.D_flag <= 1)
            err = SetFileTime(__G);
        }

#ifndef SwitchZIPITefSupportOff
    if ((MacZipMode == TomBrownZipIt1_EF) ||
        (MacZipMode == TomBrownZipIt2_EF))
        {
        if(FSpIsMacBinary(&CurrentFile))
            {
            Info(slide, 0, ((char *)slide, LoadFarString(MacBinaryMsg)));
                err = DecodeMacBinaryFile(&CurrentFile);
                printerr("close_outfile DecodeMacBinaryFile ", err, err,
                  __LINE__, __FILE__, G.filename);
            }
        }
#endif

        /* set read-only perms if needed */
    if ((err == noErr) && G.pInfo->read_only)
        {
        err = FSpSetFLock(&CurrentFile);
        printerr("FSpSetFLock",err,err,__LINE__,__FILE__,G.filename);
        }

    if (malloced_attrbuff != NULL)
        {
        attrbuff = malloced_attrbuff;
        }

} /* end function close_outfile() */




/****************************/
/* Function SetFileTime() */
/****************************/

static OSErr SetFileTime(__G)
    __GDEF
{
#ifdef USE_EF_UT_TIME
    iztimes z_utime;
    unsigned eb_izux_flg;
#endif
    OSErr err;
    CInfoPBRec      fpb;

    fpb.hFileInfo.ioNamePtr    = CurrentFile.name;
    fpb.hFileInfo.ioVRefNum    = CurrentFile.vRefNum;
    fpb.hFileInfo.ioDirID      = CurrentFile.parID;
    fpb.hFileInfo.ioFDirIndex  = 0;

    err = PBGetCatInfoSync((CInfoPBPtr)&fpb);
    printerr("PBGetCatInfoSync", err, err, __LINE__, __FILE__, G.filename);

    if ((MacZipMode == UnKnown_EF) || UseUT_ExtraField ) {

#ifdef USE_EF_UT_TIME
        eb_izux_flg = ef_scan_for_izux(G.extra_field, G.lrec.extra_field_length, 0,
                                       G.lrec.last_mod_dos_datetime, &z_utime, NULL);

        if (G.extra_field && (eb_izux_flg & EB_UT_FL_MTIME))
            {
            fpb.hFileInfo.ioFlMdDat = UnixFtime2MacFtime(z_utime.mtime);
            fpb.hFileInfo.ioFlCrDat = UnixFtime2MacFtime(z_utime.ctime);
            }

#ifdef DEBUG_TIME
            {
            struct tm *tp = gmtime(&z_utime.ctime);
            printf(
              "SetFileTime:  Unix e.f. creat. time = %d/%2d/%2d  %2d:%2d:%2d -> %lu UTC\n",
              tp->tm_year, tp->tm_mon+1, tp->tm_mday,
              tp->tm_hour, tp->tm_min, tp->tm_sec, z_utime.ctime);
            tp = gmtime(&z_utime.mtime);
            printf(
              "SetFileTime:  Unix e.f. modif. time = %d/%2d/%2d  %2d:%2d:%2d -> %lu UTC\n",
              tp->tm_year, tp->tm_mon+1, tp->tm_mday,
              tp->tm_hour, tp->tm_min, tp->tm_sec, z_utime.mtime);
            }
#endif /* DEBUG_TIME */


        else /* no Unix time field found - use dostime  */
#endif /* !USE_EF_UT_TIME */
            {
            TTrace((stderr, "SetFileTime:  using DOS-Datetime ! \n",
                    z_utime.mtime));
            fpb.hFileInfo.ioFlMdDat = dos_to_unix_time(G.lrec.last_mod_dos_datetime);
            UNIX_TO_MACOS(fpb.hFileInfo.ioFlMdDat);
            fpb.hFileInfo.ioFlCrDat = fpb.hFileInfo.ioFlMdDat;
            }

     /* Restore ioDirID field in pb which was changed by PBGetCatInfoSync */
        fpb.hFileInfo.ioDirID = CurrentFile.parID;

        if (err == noErr)
            {
            err = PBSetCatInfoSync((CInfoPBPtr)&fpb);
            printerr("PBSetCatInfoSync",err,err,__LINE__,__FILE__,G.filename);
            }
        if (err != noErr)
            Info(slide, 0x201, ((char *)slide, LoadFarString(ErrNoTimeSet),
              FnFilter1(G.filename)));
    }

return err;
} /* end function SetFileTime() */




#ifndef SFX

/************************/
/*  Function version()  */
/************************/

void version(__G)
    __GDEF
{
/*
ZCONST char Far CompiledWith[] =
               "Compiled with %s%s for %s%s%s%s.\n\n"; */

char DateTime[50];

#ifdef __MWERKS__
char CompVer[10];
sprintf(CompVer, "%x", __MWERKS__);
#endif

    sprintf(DateTime,"%s  %s",__DATE__, __TIME__);

    sprintf((char *)slide, LoadFarString(CompiledWith),

#ifdef __MWERKS__

      " Metrowerks CodeWarrior version ",CompVer,
#else
      " ", " ",
#endif

#ifdef __MC68K__
      " MC68K Processor",
#else
      " PowerPC Processor",
#endif

#ifdef __DATE__

      "\n compile time: ", DateTime, ""
#else
      "", "", ""
#endif
    );

    (*G.message)((zvoid *)&G, slide, (ulg)strlen((char *)slide), 0);

} /* end function version() */

#endif /* !SFX */





/***********************/
/* Function macmkdir() */
/***********************/

int macmkdir(char *path)
{
    OSErr err = -1;
    OSErr err_rc;
    char CompletePath[NAME_MAX], CompletePath2[NAME_MAX];
    Boolean isDirectory = false;
    short CurrentFork;
    unsigned pathlen;
    long dirID;

    AssertStr(path, path)

    GetExtraFieldData(&MacZipMode, &newExtraField);

    if (MacZipMode >= JohnnyLee_EF) {
        RfDfFilen2Real(CompletePath, G.filename, MacZipMode,
                       (newExtraField.flags & EB_M3_FL_NOCHANGE), &CurrentFork);
        if (CurrentFork == ResourceFork)
            /* don't build a 'XtraStuf.mac:' dir  */
            return 0;
    }

    if (!IgnoreEF_Macfilename)
        {
        pathlen = strlen(path);
        strcpy(CompletePath, uO.exdir);
        strcat(CompletePath, newExtraField.FullPath);
        CompletePath[pathlen] = 0x00;
        }
    else
        {
        strcpy(CompletePath, path);
        }

    GetCompletePath(CompletePath2, CompletePath, &CurrentFile, &err);
    printerr("GetCompletePath", (err != -43) && (err != -120) && (err != 0),
              err, __LINE__, __FILE__, CompletePath2);


    err = FSpGetDirectoryID(&CurrentFile, &dirID, &isDirectory);
    printerr("macmkdir FSpGetDirectoryID ", (err != -43) && (err != 0),
             err, __LINE__, __FILE__, CompletePath2);

    if (err != -43)     /* -43 = file/directory not found  */
        return 0;
    else {
        HParamBlockRec    hpbr;

        hpbr.fileParam.ioCompletion = NULL;
        hpbr.fileParam.ioNamePtr    = CurrentFile.name;
        hpbr.fileParam.ioVRefNum    = CurrentFile.vRefNum;
        hpbr.fileParam.ioDirID      = CurrentFile.parID;
        err = PBDirCreateSync(&hpbr);
        printerr("macmkdir PBDirCreateSync ", err,
                 err, __LINE__, __FILE__, CompletePath2);

        /* finally set FinderInfo */
        if  (MacZipMode >= JohnnyLee_EF)
            {
            err_rc = SetFinderInfo(__G__ (uO.D_flag <= 0),
                                   &CurrentFile, &newExtraField);
            printerr("macmkdir SetFinderInfo ", err_rc, err_rc,
                      __LINE__, __FILE__, CompletePath2);
            }
    }

    return (int)err;
} /* macmkdir */




/**********************/
/* Function macopen() */
/**********************/

short macopen(char *sz, short nFlags)
{
    OSErr   err;
    char    chPerms = (!nFlags) ? fsRdPerm : fsRdWrPerm;
    short   nFRefNum;

    AssertStr(sz, sz)

    /* we only need the filespec of the zipfile;
       filespec of the other files (to be extracted) will be
       determined by open_outfile() */
    if (OpenZipFile)
        {
        char CompletePath[NAME_MAX];
        FSSpec zipfile;
        GetCompletePath(CompletePath, sz, &zipfile, &err);
        printerr("GetCompletePath", (err != -43) && (err != 0),
                  err, __LINE__, __FILE__, sz);
        if (CheckMountedVolumes(CompletePath) > 1)
            DoWarnUserDupVol(CompletePath);
        err = HOpen(zipfile.vRefNum, zipfile.parID, zipfile.name,
                    chPerms, &nFRefNum);
        printerr("Zipfile HOpen", err, err, __LINE__, __FILE__, sz);
        OpenZipFile = false;
        }
    else   /* open zipfile entries  */
        {
        if (nFlags > 1)
            {
            err = HOpenRF(CurrentFile.vRefNum, CurrentFile.parID, CurrentFile.name,
                      chPerms, &nFRefNum);
            printerr("HOpenRF", (err != -43) && (err != 0) && (err != -54),
                 err, __LINE__, __FILE__, sz);
            }
        else
            {
            err = HOpen(CurrentFile.vRefNum, CurrentFile.parID, CurrentFile.name,
                    chPerms, &nFRefNum);
            printerr("HOpen", (err != -43) && (err != 0),
                 err, __LINE__, __FILE__, sz);
            }
        }

    if ( err || (nFRefNum == 1) )
        {
        printerr("macopen", err, err, __LINE__, __FILE__,
                 (char *) CurrentFile.name);
        return -1;
        }
    else
        {
        if ( nFlags )
            SetEOF( nFRefNum, 0 );
        return nFRefNum;
        }
}





/***********************/
/* Function maccreat() */
/***********************/

short maccreat(char *sz)
{
    OSErr   err;
    char scriptTag = newExtraField.fpb.hFileInfo.ioFlXFndrInfo.fdScript;
    static char Num = 0;

    sz = sz;

    /* Set fdScript in FXInfo
     * The negative script constants (smSystemScript, smCurrentScript,
     * and smAllScripts) don't make sense on disk.  So only use scriptTag
     * if scriptTag >= smRoman (smRoman is 0).
     * fdScript is valid if high bit is set (see IM-6, page 9-38)
     */
    scriptTag = (scriptTag >= smRoman) ?
                ((char)scriptTag | (char)0x80) : (smRoman);
    newExtraField.fpb.hFileInfo.ioFlXFndrInfo.fdScript = scriptTag;

    err = FSpCreate(&CurrentFile,
                    newExtraField.fpb.hFileInfo.ioFlFndrInfo.fdCreator,
                    newExtraField.fpb.hFileInfo.ioFlFndrInfo.fdType,
                    newExtraField.fpb.hFileInfo.ioFlXFndrInfo.fdScript);

    /* -37 = bad filename; make the filename shorter and try again  */
    /* filename must not be longer than 32 chars */
    if (err == -37)
        {
        strcpy((char *)CurrentFile.name,
                MakeFilenameShorter(P2CStr(CurrentFile.name)));
        Info(slide, 0x401, ((char *)slide, LoadFarString(FilenameToLong),
          FnFilter1((char *)CurrentFile.name)));
        C2PStr((char *)CurrentFile.name);
        err = FSpCreate(&CurrentFile,
                    newExtraField.fpb.hFileInfo.ioFlFndrInfo.fdCreator,
                    newExtraField.fpb.hFileInfo.ioFlFndrInfo.fdType,
                    newExtraField.fpb.hFileInfo.ioFlXFndrInfo.fdScript);
        }

    err = printerr("FSpCreate maccreat ", (err != -48) && (err != 0),
                   err, __LINE__, __FILE__, G.filename);

    if (err == noErr)
        return noErr;
    else
        return -1;
}





/**********************/
/* Function macread() */
/**********************/

short macread(short nFRefNum, char *pb, unsigned cb)
{
    long    lcb = cb;

    (void)FSRead( nFRefNum, &lcb, pb );
    return (short)lcb;
}




/***********************/
/* Function macwrite() */
/***********************/

long macwrite(short nFRefNum, char *pb, unsigned cb)
{
    long    lcb = cb;
    OSErr   err = 0;
    FILE    *stream;

    if ( (nFRefNum == 1) || (nFRefNum == 2) )
        {
        stream = (nFRefNum == 1 ? stdout : stderr);
        pb[cb] = '\0';           /* terminate C-string */
                                 /* assumes writable buffer (e.g., slide[]) */
                                 /* with room for one more char at end of buf */
        lcb = fprintf(stream, "%s", pb);
        }
    else
        err = FSWrite( nFRefNum, &lcb, pb );

    if (err != 0)
        {
        errno = ERANGE;
        return -1;
        }

    return (long)lcb;
}





/***********************/
/* Function macclose() */
/***********************/

short macclose(short nFRefNum)
{
OSErr err;

err = FSClose( nFRefNum );
printerr("macclose FSClose ",err,err, __LINE__,__FILE__,G.filename);


return err;
}





/***********************/
/* Function maclseek() */
/***********************/

long maclseek(short nFRefNum, long lib, short nMode)
{
    ParamBlockRec   pbr;

    if (nMode == SEEK_SET)
        nMode = fsFromStart;
    else if (nMode == SEEK_CUR)
        nMode = fsFromMark;
    else if (nMode == SEEK_END)
        nMode = fsFromLEOF;
    pbr.ioParam.ioRefNum = nFRefNum;
    pbr.ioParam.ioPosMode = nMode;
    pbr.ioParam.ioPosOffset = lib;
    (void)PBSetFPosSync(&pbr);
    return pbr.ioParam.ioPosOffset;
}



static void BuildMacFilename(void)
{
char CompletePath[NAME_MAX];
char ArchiveDir[NAME_MAX];
unsigned exdirlen = strlen(uO.exdir);
short CurrentFork;
OSErr err;

GetExtraFieldData(&MacZipMode, &newExtraField);

if (MacZipMode >= JohnnyLee_EF)
    {
    if (IgnoreEF_Macfilename)
        {
        strcpy(ArchiveDir, &G.filename[exdirlen+1]);
        G.filename[exdirlen+1] = '\0';
        RfDfFilen2Real(ArchiveDir, ArchiveDir, MacZipMode,
                      (newExtraField.flags & EB_M3_FL_DATFRK), &CurrentFork);
        strcat(G.filename, ArchiveDir);
        }
    else
        {        /* use the filename of mac extra-field */
        G.filename[exdirlen] = '\0';  /* cut resource-path */
        strcat(G.filename,newExtraField.FullPath);
        }
    }

GetCompletePath(CompletePath, G.filename, &CurrentFile, &err);
printerr("GetCompletePath BuildMacFilename ", (err != -43) && (err != 0),
             err, __LINE__, __FILE__, CompletePath);

err = GetVolFileSystemID(C2PStr(CompletePath), CurrentFile.vRefNum,
                         &fileSystemID);
printerr("GetVolFileSystemID BuildMacFilename ", err, err, __LINE__, __FILE__,
          CompletePath);

if (MacZipMode == TomBrownZipIt1_EF)
    {
    memcpy(CurrentFile.name, newExtraField.filename, newExtraField.filename[0]+1);
    CurrentFile.name[0] = CurrentFile.name[0] - 1;
    }

}




/* The following functions are dealing with the extra-field handling, only. */

/****************************/
/* Function SetFinderInfo() */
/****************************/

static OSErr SetFinderInfo(__GPRO__ int SetTime, FSSpec *spec, MACINFO *mi)
{
    OSErr err;
    CInfoPBRec      fpb;

    fpb.hFileInfo.ioNamePtr   = (StringPtr) &(spec->name);
    fpb.hFileInfo.ioVRefNum   = spec->vRefNum;
    fpb.hFileInfo.ioDirID     = spec->parID;
    fpb.hFileInfo.ioFDirIndex = 0;

    err = PBGetCatInfoSync(&fpb);
    printerr("PBGetCatInfo SetFinderInfo ", err, err,
             __LINE__, __FILE__, G.filename);

    if  ((MacZipMode == JohnnyLee_EF) || (MacZipMode == NewZipMode_EF))
    {
        /* skip restoring time stamps on user's request */
        if (SetTime && !UseUT_ExtraField) {
            fpb.hFileInfo.ioFlCrDat = mi->fpb.hFileInfo.ioFlCrDat;
            fpb.hFileInfo.ioFlMdDat = mi->fpb.hFileInfo.ioFlMdDat;
        }

        fpb.hFileInfo.ioFlFndrInfo   = mi->fpb.hFileInfo.ioFlFndrInfo;
    }

    if (MacZipMode == NewZipMode_EF)
    {
        if (uO.E_flag) PrintMacExtraInfo(mi);
        fpb.hFileInfo.ioFlXFndrInfo  = mi->fpb.hFileInfo.ioFlXFndrInfo;

        fpb.hFileInfo.ioFVersNum  = mi->fpb.hFileInfo.ioFVersNum;
        fpb.hFileInfo.ioACUser    = mi->fpb.hFileInfo.ioACUser;

        /* skip restoring time stamps on user's request */
        if (SetTime && !UseUT_ExtraField) {
            fpb.hFileInfo.ioFlBkDat = mi->fpb.hFileInfo.ioFlBkDat;
#ifdef USE_EF_UT_TIME
            if (!(mi->flags & EB_M3_FL_NOUTC))
                {
#ifdef DEBUG_TIME
            {
            printf("\nSetFinderInfo:  Mac modif: %lu local -> UTOffset: "\
                   "%d before AdjustForTZmoveMac\n",
              fpb.hFileInfo.ioFlCrDat, mi->Cr_UTCoffs);
            }
#endif /* DEBUG_TIME */
                fpb.hFileInfo.ioFlCrDat =
                  AdjustForTZmoveMac(fpb.hFileInfo.ioFlCrDat, mi->Cr_UTCoffs);
                fpb.hFileInfo.ioFlMdDat =
                  AdjustForTZmoveMac(fpb.hFileInfo.ioFlMdDat, mi->Md_UTCoffs);
                fpb.hFileInfo.ioFlBkDat =
                  AdjustForTZmoveMac(fpb.hFileInfo.ioFlBkDat, mi->Bk_UTCoffs);
#ifdef DEBUG_TIME
            {
            printf("SetFinderInfo:  Mac modif: %lu local -> UTOffset: "\
                   "%d after AdjustForTZmoveMac\n",
              fpb.hFileInfo.ioFlCrDat, mi->Cr_UTCoffs);
            }
#endif /* DEBUG_TIME */

                }
#endif /* USE_EF_UT_TIME */
        }

        if ((mi->FinderComment) &&
           (fileSystemID == HFS_fileSystem)) {
            C2PStr(mi->FinderComment);
            err = FSpDTSetComment(spec, (unsigned char *) mi->FinderComment);
            printerr("FSpDTSetComment:",err , err,
                     __LINE__, __FILE__, mi->FullPath);
        }
    }

    /* Restore ioDirID field in pb which was changed by PBGetCatInfo */
    fpb.hFileInfo.ioDirID = spec->parID;
    err = PBSetCatInfoSync(&fpb);

    return err;
} /* SetFinderInfo() */




/*
** Scan the extra fields in extra_field, and look for a MacOS EF; return a
** pointer to that EF, or NULL if it's not there.
*/
static uch *scanMacOSexfield(uch *ef_ptr, unsigned ef_len,
                             short *MacZipMode)
{
    while (ef_ptr != NULL && ef_len >= EB_HEADSIZE) {
        unsigned eb_id  = makeword(EB_ID  + ef_ptr);
        unsigned eb_len = makeword(EB_LEN + ef_ptr);

        if (eb_len > (ef_len - EB_HEADSIZE)) {
            Trace((stderr,
              "scanMacOSexfield: block length %u > rest ef_size %u\n", eb_len,
              ef_len - EB_HEADSIZE));
            break;
        }

        if (isMacOSexfield(eb_id, eb_len, MacZipMode)) {
            return ef_ptr;
        }

        ef_ptr += (eb_len + EB_HEADSIZE);
        ef_len -= (eb_len + EB_HEADSIZE);
    }

    return NULL;
}




static Boolean isMacOSexfield(unsigned id, unsigned size, short *MacZipMode)
{
size = size;

    switch (id)
        {
        case EF_ZIPIT:
            {           /* we do not (yet) support ZipIt's format completely */
            *MacZipMode = TomBrownZipIt1_EF;
            IgnoreEF_Macfilename = true;
            return true;
            }

        case EF_ZIPIT2:
            {           /* we do not (yet) support ZipIt's format completely */
            *MacZipMode = TomBrownZipIt2_EF;
            IgnoreEF_Macfilename = true;
            return true;
            }

        case EF_MAC3:
            {           /* the new maczip format */
            *MacZipMode = NewZipMode_EF;
            IgnoreEF_Macfilename = false;
            return true;
            }

        case EF_JLMAC:
            {           /* Johnny Lee's old maczip format */
            *MacZipMode = JohnnyLee_EF;
            IgnoreEF_Macfilename = true;
            return true;
            }

        default:
            {           /* any other format */
            *MacZipMode = UnKnown_EF;
            IgnoreEF_Macfilename = true;
            return false;
            }
        }

    return false;
}




/*
** Return a unsigned long from a four-byte sequence
** in big endian format
*/

ulg makePPClong(ZCONST uch *sig)
{
    return (((ulg)sig[0]) << 24)
         + (((ulg)sig[1]) << 16)
         + (((ulg)sig[2]) << 8)
         +  ((ulg)sig[3]);
}




/*
** Return a unsigned short from a two-byte sequence
** in big endian format
*/

ush makePPCword(ZCONST uch *b)
{
    return (ush)((b[0] << 8) | b[1]);
}




/*
** Print mac extra-field
**
*/

static void PrintMacExtraInfo(MACINFO *mi)
{
#define MY_FNDRINFO fpb.hFileInfo.ioFlFndrInfo
    DateTimeRec  MacTime;
    static ZCONST char space[] = "                                    ";
    static ZCONST char line[]  = "------------------------------------"\
                                 "------------------------------";

    printf("\n\n%s", line);

    printf("\nFullPath      = [%s]", mi->FullPath);
    printf("\nFinderComment = [%s]", mi->FinderComment);
    printf("\nText Encoding Base (Filename)       \"%s\" \n",
        PrintTextEncoding(mi->fpb.hFileInfo.ioFlXFndrInfo.fdScript));

    printf("\nExtraField Flags :                  %s  0x%x  %4d",
             sBit2Str(mi->flags), mi->flags, mi->flags);

    printf("\n%sExtra Field is %s", space,
           (mi->flags & EB_M3_FL_UNCMPR ?
            "Uncompressed" : "Compressed"));
    printf("\n%sFile Dates are in %u Bit", space,
           (mi->flags & EB_M3_FL_TIME64 ? 64 : 32));
    printf("\n%sFile UTC time adjustments are %ssupported", space,
           (mi->flags & EB_M3_FL_NOUTC ? "not " : ""));
    printf("\n%sFile Name is %schanged", space,
           (mi->flags & EB_M3_FL_NOCHANGE ? "not " : ""));
    printf("\n%sFile is a %s\n", space,
           (mi->flags & EB_M3_FL_DATFRK ?
            "Datafork" : "Resourcefork"));

    /* not all type / creator codes are printable */
    if (isprint((char)(mi->MY_FNDRINFO.fdType >> 24)) &&
        isprint((char)(mi->MY_FNDRINFO.fdType >> 16)) &&
        isprint((char)(mi->MY_FNDRINFO.fdType >> 8)) &&
        isprint((char)mi->MY_FNDRINFO.fdType))
    {
        printf("\nFile Type =                         [%c%c%c%c]  0x%lx",
            (char)(mi->MY_FNDRINFO.fdType >> 24),
            (char)(mi->MY_FNDRINFO.fdType >> 16),
            (char)(mi->MY_FNDRINFO.fdType >> 8),
            (char)(mi->MY_FNDRINFO.fdType),
            mi->MY_FNDRINFO.fdType);
    }
    else
    {
        printf("\nFile Type =                                     0x%lx",
            mi->MY_FNDRINFO.fdType);
    }

    if (isprint((char)(mi->MY_FNDRINFO.fdCreator >> 24)) &&
        isprint((char)(mi->MY_FNDRINFO.fdCreator >> 16)) &&
        isprint((char)(mi->MY_FNDRINFO.fdCreator >> 8)) &&
        isprint((char)mi->MY_FNDRINFO.fdCreator))
    {
        printf("\nFile Creator =                      [%c%c%c%c]  0x%lx",
            (char)(mi->MY_FNDRINFO.fdCreator >> 24),
            (char)(mi->MY_FNDRINFO.fdCreator >> 16),
            (char)(mi->MY_FNDRINFO.fdCreator >> 8),
            (char)(mi->MY_FNDRINFO.fdCreator),
            mi->MY_FNDRINFO.fdCreator);
    }
    else
    {
        printf("\nFile Creator =                                  0x%lx",
            mi->MY_FNDRINFO.fdCreator);
    }

    printf("\n\nDates (local time of archiving location):");
    SecondsToDate(mi->fpb.hFileInfo.ioFlCrDat, &MacTime);
    printf("\n    Created  =                      %4d/%2d/%2d %2d:%2d:%2d  ",
           MacTime.year, MacTime.month, MacTime.day,
           MacTime.hour, MacTime.minute, MacTime.second);
    SecondsToDate(mi->fpb.hFileInfo.ioFlMdDat, &MacTime);
    printf("\n    Modified =                      %4d/%2d/%2d %2d:%2d:%2d  ",
           MacTime.year, MacTime.month, MacTime.day,
           MacTime.hour, MacTime.minute, MacTime.second);
    SecondsToDate(mi->fpb.hFileInfo.ioFlBkDat, &MacTime);
    printf("\n    Backup   =                      %4d/%2d/%2d %2d:%2d:%2d  ",
        MacTime.year, MacTime.month, MacTime.day,
        MacTime.hour, MacTime.minute, MacTime.second);

    if (!(mi->flags & EB_M3_FL_NOUTC)) {
        printf("\nGMT Offset of Creation time  =      %4ld sec  %2d h",
          mi->Cr_UTCoffs, (int)mi->Cr_UTCoffs / (60 * 60));
        printf("\nGMT Offset of Modification time  =  %4ld sec  %2d h",
          mi->Md_UTCoffs, (int)mi->Md_UTCoffs / (60 * 60));
        printf("\nGMT Offset of Backup time  =        %4ld sec  %2d h",
          mi->Bk_UTCoffs, (int)mi->Bk_UTCoffs / (60 * 60));
    }

    printf("\n\nFinder Flags :                      %s  0x%x  %4d",
        sBit2Str(mi->MY_FNDRINFO.fdFlags),
        mi->MY_FNDRINFO.fdFlags,
        mi->MY_FNDRINFO.fdFlags);

    printf("\nFinder Icon Position =              X: %4d",
        mi->MY_FNDRINFO.fdLocation.h);

    printf("\n                                    Y: %4d",
        mi->MY_FNDRINFO.fdLocation.v);

    printf("\n\nText Encoding Base (System/MacZip)  \"%s\"",
        PrintTextEncoding(mi->TextEncodingBase));

    printf("\n%s\n", line);
#undef MY_FNDRINFO
}




/*
** Decode mac extra-field and assign the data to the structure
**
*/

static Boolean GetExtraFieldData(short *MacZipMode, MACINFO *mi)
{
uch *ptr;
int  retval = PK_OK;

ptr = scanMacOSexfield(G.extra_field, G.lrec.extra_field_length, MacZipMode);

/* MacOS is no preemptive OS therefore do some (small) event-handling */
UserStop();

if (uO.J_flag)
    {
    *MacZipMode = UnKnown_EF;
    IgnoreEF_Macfilename = true;
    return false;
    }

if (ptr != NULL)
{   /*   Collect the data from the extra field buffer. */
    mi->header    = makeword(ptr);    ptr += 2;
    mi->data      = makeword(ptr);    ptr += 2;

    switch (*MacZipMode)
        {
        case NewZipMode_EF:
           {
            mi->size      =  makelong(ptr); ptr += 4;
            mi->flags     =  makeword(ptr); ptr += 2;
                             /* Type/Creator are always uncompressed */
            mi->fpb.hFileInfo.ioFlFndrInfo.fdType    = makePPClong(ptr);
            ptr += 4;
            mi->fpb.hFileInfo.ioFlFndrInfo.fdCreator = makePPClong(ptr);
            ptr += 4;

            if (!(mi->flags & EB_M3_FL_UNCMPR))
                {

                retval = memextract(__G__ attrbuff, mi->size, ptr,
                                    mi->data - EB_MAC3_HLEN);

                if (retval != PK_OK)  /* error uncompressing attributes */
                    {
                    Info(slide, 0x201, ((char *)slide,
                         LoadFarString(ErrUncmpEF)));
                    *MacZipMode = UnKnown_EF;
                    return false;     /* EF-Block unusable, ignore it */
                    }

                }
             else
                {   /* file attributes are uncompressed */
                attrbuff = ptr;
                }

            DecodeMac3ExtraField(attrbuff, mi);

            return true;
            break;
            }

        case JohnnyLee_EF:
            {
            if (strncmp((char *)ptr, "JLEE", 4) == 0)
                {   /* Johnny Lee's old MacZip e.f. was found */
                attrbuff  = ptr + 4;
                DecodeJLEEextraField(attrbuff, mi);
                return true;
                }
            else
                {   /* second signature did not match, ignore EF block */
                *MacZipMode = UnKnown_EF;
                return false;
                }
            break;
            }


        case TomBrownZipIt1_EF:
        case TomBrownZipIt2_EF:
            {
            if (strncmp((char *)ptr, "ZPIT", 4) == 0)
                {   /* Johnny Lee's old MacZip e.f. was found */
                attrbuff  = ptr + 4;
                DecodeZPITextraField(attrbuff, mi);
                return true;
                }
            else
                {   /* second signature did not match, ignore EF block */
                *MacZipMode = UnKnown_EF;
                return false;
                }
            break;
            }


        default:
            {  /* just to make sure */
            *MacZipMode = UnKnown_EF;
            IgnoreEF_Macfilename = true;
            return false;
            break;
            }
        }
}  /* if (ptr != NULL)  */

/* no Mac extra field was found */
return false;
}




/*
** Assign the new Mac3 Extra-Field to the structure
**
*/

static void DecodeMac3ExtraField(ZCONST uch *buff, MACINFO *mi)
{               /* extra-field info of the new MacZip implementation */
                /* compresssed extra-field starts here (if compressed) */

Assert_it(buff, "", "");

mi->fpb.hFileInfo.ioFlFndrInfo.fdFlags        =  makeword(buff); buff += 2;
mi->fpb.hFileInfo.ioFlFndrInfo.fdLocation.v   =  makeword(buff); buff += 2;
mi->fpb.hFileInfo.ioFlFndrInfo.fdLocation.h   =  makeword(buff); buff += 2;
mi->fpb.hFileInfo.ioFlFndrInfo.fdFldr         =  makeword(buff); buff += 2;

mi->fpb.hFileInfo.ioFlXFndrInfo.fdIconID      =  makeword(buff); buff += 2;
mi->fpb.hFileInfo.ioFlXFndrInfo.fdReserved[0] =  makeword(buff); buff += 2;
mi->fpb.hFileInfo.ioFlXFndrInfo.fdReserved[1] =  makeword(buff); buff += 2;
mi->fpb.hFileInfo.ioFlXFndrInfo.fdReserved[2] =  makeword(buff); buff += 2;
mi->fpb.hFileInfo.ioFlXFndrInfo.fdScript      = *buff;           buff += 1;
mi->fpb.hFileInfo.ioFlXFndrInfo.fdXFlags      = *buff;           buff += 1;
mi->fpb.hFileInfo.ioFlXFndrInfo.fdComment     =  makeword(buff); buff += 2;
mi->fpb.hFileInfo.ioFlXFndrInfo.fdPutAway     =  makelong(buff); buff += 4;

mi->fpb.hFileInfo.ioFVersNum                  = *buff;           buff += 1;
mi->fpb.hFileInfo.ioACUser                    = *buff;           buff += 1;

/*
This implementation does not use the 64 bit time values, therefore
use the UT extra field instead
*/
if (mi->flags & EB_M3_FL_TIME64)
    {
    Info(slide, 0x201, ((char *)slide, LoadFarString(No64Time)));
    UseUT_ExtraField = true;
    buff += 24; /* jump over the date values */
    }
else
    {
    UseUT_ExtraField = false;
    mi->fpb.hFileInfo.ioFlCrDat   =  makelong(buff); buff += 4;
    mi->fpb.hFileInfo.ioFlMdDat   =  makelong(buff); buff += 4;
    mi->fpb.hFileInfo.ioFlBkDat   =  makelong(buff); buff += 4;
    }

if (!(mi->flags & EB_M3_FL_NOUTC))
    {
    mi->Cr_UTCoffs =  makelong(buff); buff += 4;
    mi->Md_UTCoffs =  makelong(buff); buff += 4;
    mi->Bk_UTCoffs =  makelong(buff); buff += 4;
    }

/* TextEncodingBase type & values */
/* (values 0-32 correspond to the Script Codes defined in "Inside Macintosh",
    Text pages 6-52 and 6-53) */
mi->TextEncodingBase =  makeword(buff); buff += 2;

if (mi->TextEncodingBase >= kTextEncodingUnicodeV1_1)
    {
    Info(slide, 0x201, ((char *)slide, LoadFarString(NoUniCode)));
    IgnoreEF_Macfilename = true;
    }

mi->FullPath      = (char *)buff; buff += strlen(mi->FullPath) + 1;
mi->FinderComment = (char *)buff; buff += strlen(mi->FinderComment) + 1;

if (uO.i_flag) IgnoreEF_Macfilename = true;

}




/*
** Assign the new JLEE Extra-Field to the structure
**
*/

static void DecodeJLEEextraField(ZCONST uch *buff, MACINFO *mi)
{ /*  extra-field info of Johnny Lee's old MacZip  */

Assert_it(buff, "", "");

mi->fpb.hFileInfo.ioFlFndrInfo.fdType       = makePPClong(buff); buff += 4;
mi->fpb.hFileInfo.ioFlFndrInfo.fdCreator    = makePPClong(buff); buff += 4;
mi->fpb.hFileInfo.ioFlFndrInfo.fdFlags      = makePPCword(buff); buff += 2;
mi->fpb.hFileInfo.ioFlFndrInfo.fdLocation.v = makePPCword(buff); buff += 2;
mi->fpb.hFileInfo.ioFlFndrInfo.fdLocation.h = makePPCword(buff); buff += 2;
mi->fpb.hFileInfo.ioFlFndrInfo.fdFldr       = makePPCword(buff); buff += 2;

mi->fpb.hFileInfo.ioFlCrDat                =  makePPClong(buff); buff += 4;
mi->fpb.hFileInfo.ioFlMdDat                =  makePPClong(buff); buff += 4;
mi->flags                                  =  makePPClong(buff); buff += 4;

newExtraField.fpb.hFileInfo.ioFlXFndrInfo.fdScript = smSystemScript;
}




/*
** Assign the new JLEE Extra-Field to the structure
**
*/

static void DecodeZPITextraField(ZCONST uch *buff, MACINFO *mi)
{ /*  extra-field info of Johnny Lee's old MacZip  */
unsigned char filelen;

Assert_it(buff, "", "");

#ifdef SwitchZIPITefSupportOff
MacZipMode = UnKnown_EF;
Info(slide, 0x221, ((char *)slide,LoadFarString(ZitIt_EF)));
return;
#endif

if (MacZipMode == TomBrownZipIt1_EF)
    {
    filelen = *buff;
    newExtraField.filename = buff;
    buff += 1;
    buff += filelen;
    mi->fpb.hFileInfo.ioFlFndrInfo.fdType       = makePPClong(buff);
    buff += 4;
    mi->fpb.hFileInfo.ioFlFndrInfo.fdCreator    = makePPClong(buff);
    buff += 4;
    }
else   /*  if (MacZipMode == TomBrownZipIt2_EF)  */
    {
    mi->fpb.hFileInfo.ioFlFndrInfo.fdType       = makePPClong(buff);
    buff += 4;
    mi->fpb.hFileInfo.ioFlFndrInfo.fdCreator    = makePPClong(buff);
    buff += 4;
    }

newExtraField.fpb.hFileInfo.ioFlXFndrInfo.fdScript = smSystemScript;
}



/*
** Return char* to describe the text encoding
**
*/

static char *PrintTextEncoding(short script)
{
char *info;
static char buffer[14];
/* TextEncodingBase type & values */
/* (values 0-32 correspond to the Script Codes defined in
   Inside Macintosh: Text pages 6-52 and 6-53 */

switch (script) {               /* Mac OS encodings*/
    case kTextEncodingMacRoman:         info = "Roman";             break;
    case kTextEncodingMacJapanese:      info = "Japanese";          break;
    case kTextEncodingMacChineseTrad:   info = "ChineseTrad";       break;
    case kTextEncodingMacKorean:        info = "Korean";            break;
    case kTextEncodingMacArabic:        info = "Arabic";            break;
    case kTextEncodingMacHebrew:        info = "Hebrew";            break;
    case kTextEncodingMacGreek:         info = "Greek";             break;
    case kTextEncodingMacCyrillic:      info = "Cyrillic";          break;
    case kTextEncodingMacDevanagari:    info = "Devanagari";        break;
    case kTextEncodingMacGurmukhi:      info = "Gurmukhi";          break;
    case kTextEncodingMacGujarati:      info = "Gujarati";          break;
    case kTextEncodingMacOriya:         info = "Oriya";             break;
    case kTextEncodingMacBengali:       info = "Bengali";           break;
    case kTextEncodingMacTamil:         info = "Tamil";             break;
    case kTextEncodingMacTelugu:        info = "Telugu";            break;
    case kTextEncodingMacKannada:       info = "Kannada";           break;
    case kTextEncodingMacMalayalam:     info = "Malayalam";         break;
    case kTextEncodingMacSinhalese:     info = "Sinhalese";         break;
    case kTextEncodingMacBurmese:       info = "Burmese";           break;
    case kTextEncodingMacKhmer:         info = "Khmer";             break;
    case kTextEncodingMacThai:          info = "Thai";              break;
    case kTextEncodingMacLaotian:       info = "Laotian";           break;
    case kTextEncodingMacGeorgian:      info = "Georgian";          break;
    case kTextEncodingMacArmenian:      info = "Armenian";          break;
    case kTextEncodingMacChineseSimp:   info = "ChineseSimp";       break;
    case kTextEncodingMacTibetan:       info = "Tibetan";           break;
    case kTextEncodingMacMongolian:     info = "Mongolian";         break;
    case kTextEncodingMacEthiopic:      info = "Ethiopic";          break;
    case kTextEncodingMacCentralEurRoman: info = "CentralEurRoman"; break;
    case kTextEncodingMacVietnamese:    info = "Vietnamese";        break;
    case kTextEncodingMacExtArabic:     info = "ExtArabic";         break;

    case kTextEncodingUnicodeV1_1:      info = "Unicode V 1.1";     break;
    case kTextEncodingUnicodeV2_0:      info = "Unicode V 2.0";     break;

    default:  {
        sprintf(buffer,"Code: 0x%x",(short) script);
        info = buffer;
        break;
        }
    }

return info;
}



/*
** Init Globals
**
*/

void   MacGlobalsInit(__GPRO)
{
newExtraField.FullPath      = NULL;
newExtraField.FinderComment = NULL;

OpenZipFile = true;

MacZipMode = UnKnown_EF;
IgnoreEF_Macfilename = true;

if (malloced_attrbuff == NULL)
    {
    /* make room for extra-field */
    attrbuff = (uch *)malloc(EB_MAX_OF_VARDATA);

    if (attrbuff == NULL)
        {             /* No memory to uncompress attributes */
        Info(slide, 0x201, ((char *)slide, LoadFarString(OutOfMemEF)));
        exit(PK_MEM);
        }
    else
        {
        malloced_attrbuff = attrbuff;
        }
    }
else
    {
    attrbuff = malloced_attrbuff;
    }

}
