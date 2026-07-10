/*
  Copyright (c) 1990-2007 Info-ZIP.  All rights reserved.

  See the accompanying file LICENSE, version 2000-Apr-09 or later
  (the contents of which are also included in unzip.h) for terms of use.
  If, for some reason, all these files are missing, the Info-ZIP license
  also may be found at:  ftp://ftp.info-zip.org/pub/infozip/license.html
*/
/*---------------------------------------------------------------------------

  human68k.c

  Human68k-specific routines for use with Info-ZIP's UnZip 5.41 and later.

  Contains:  do_wild()
             mapattr()
             mapname()
             checkdir()
             close_outfile()
             stamp_file()                   (TIMESTAMP only)
             version()
             main()                         (for UnZipSFX)

  ---------------------------------------------------------------------------*/


#include <dirent.h>
#include <string.h>
#include <sys/dos.h>
#include <sys/xunistd.h>
#ifdef HAVE_TWONCALL_H
#include <twoncall.h>
#endif
#define UNZIP_INTERNAL
#include "unzip.h"

#if defined (SFX) && defined (MAIN)
#include <sys/xstart.h>
int MAIN(int argc, char *argv[]);
#endif

static void map2fat(char *pathcomp, char *last_dot);
static char *trunc_name(char *name, int maxlen);

static int created_dir;        /* used in mapname(), checkdir() */
static int renamed_fullpath;   /* ditto */

static char multi_period, special_char;

#ifndef SFX

/**********************/
/* Function do_wild() */
/**********************/

char *do_wild(__G__ wildspec)
    __GDEF
    ZCONST char *wildspec;  /* only used first time on a given dir */
{
    static DIR *wild_dir = (DIR *)NULL;
    static ZCONST char *wildname;
    static char *dirname, matchname[FILNAMSIZ];
    static int notfirstcall=FALSE, have_dirname, dirnamelen;
    struct dirent *file;

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
            wild_dir = (DIR *)NULL;
            return matchname;
        }

        /* break the wildspec into a directory part and a wildcard filename */
        if ((wildname = strrchr(wildspec, '/')) == NULL) {
            dirname = ".";
            dirnamelen = 1;
            have_dirname = FALSE;
            wildname = wildspec;
        } else {
            ++wildname;     /* point at character after '/' */
            dirnamelen = wildname - wildspec;
            if ((dirname = (char *)malloc(dirnamelen+1)) == NULL) {
                Info(slide, 1, ((char *)slide,
                  "warning:  cannot allocate wildcard buffers\n"));
                strcpy(matchname, wildspec);
                return matchname;   /* but maybe filespec was not a wildcard */
            }
            strncpy(dirname, wildspec, dirnamelen);
            dirname[dirnamelen] = '\0';   /* terminate for strcpy below */
            have_dirname = TRUE;
        }
        Trace((stderr, "do_wild:  dirname = [%s]\n", FnFilter1(dirname)));

        if ((wild_dir = opendir(dirname)) != (DIR *)NULL) {
            while ((file = readdir(wild_dir)) != (struct dirent *)NULL) {
                Trace((stderr, "do_wild:  readdir returns %s\n",
                  FnFilter1(file->d_name)));
                if (file->d_name[0] == '.' && wildname[0] != '.')
                    continue; /* Unix:  '*' and '?' do not match leading dot */
                if (match(file->d_name, wildname, 0 WISEP) && /* 0=case sens.*/
                    /* skip "." and ".." directory entries */
                    strcmp(file->d_name, ".") && strcmp(file->d_name, "..")) {
                    Trace((stderr, "do_wild:  match() succeeds\n"));
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
    while ((file = readdir(wild_dir)) != (struct dirent *)NULL) {
        Trace((stderr, "do_wild:  readdir returns %s\n",
          FnFilter1(file->d_name)));
        if (file->d_name[0] == '.' && wildname[0] != '.')
            continue;   /* Unix:  '*' and '?' do not match leading dot */
        if (match(file->d_name, wildname, 0 WISEP)) {   /* 0 == case sens. */
            Trace((stderr, "do_wild:  match() succeeds\n"));
            if (have_dirname) {
                /* strcpy(matchname, dirname); */
                strcpy(matchname+dirnamelen, file->d_name);
            } else
                strcpy(matchname, file->d_name);
            return matchname;
        }
    }

    closedir(wild_dir);     /* have read at least one entry; nothing left */
    wild_dir = (DIR *)NULL;
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
    ulg  tmp = G.crec.external_file_attributes;

    switch (G.pInfo->hostnum) {
        case UNIX_:
            if (tmp & 0xff)
                break;
            /* fall through */
        case VMS_:
        case ACORN_:
        case ATARI_:
        case ATHEOS_:
        case BEOS_:
        case QDOS_:
            G.pInfo->file_attr = _mode2dos(tmp >> 16);
            return 0;
        default:
            break;
    }

    /* set archive bit (file is not backed up) */
    if((tmp & 0x08) == 0)
        tmp |= 0x20;
    G.pInfo->file_attr = tmp & 0xff;
    return 0;

} /* end function mapattr() */





/**********************/
/* Function mapname() */
/**********************/

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
    char *last_dot=(char *)NULL;   /* last dot */
    int error = MPN_OK;
    register unsigned workch;      /* hold the character being tested */

#ifdef HAVE_TWONCALL_H
    static char twentyone_flag;

    /* Get TwentyOne options */
    if (twentyone_flag == 0) {
        twentyone_flag++;
        if (GetTwentyOneID () == TWON_ID) {
            int flags = GetTwentyOneOptions ();

            if (flags & (1 << TWON_PERIOD_BIT))
                multi_period = TRUE;
            if (flags & (1 << TWON_SPECIAL_BIT))
                special_char = TRUE;
        }
    }
#endif

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
        if ((G.filename[0] == '/')
         || (isalpha(G.filename[0]) && G.filename[1] == ':')) {
            /* user gave full pathname:  don't prepend rootpath */
            renamed_fullpath = TRUE;
        }
    }

    if ((error = checkdir(__G__ (char *)NULL, INIT)) != 0)
        return error;           /* initialize path buffer, unless no memory */

    *pathcomp = '\0';           /* initialize translation buffer */
    pp = pathcomp;              /* point to translation buffer */
    if (uO.jflag)               /* junking directories */
        cp = (char *)strrchr(G.filename, '/');
    if (cp == (char *)NULL)     /* no '/' or not junking dirs */
        cp = G.filename;        /* point to internal zipfile-member pathname */
    else
        ++cp;                   /* point to start of last component of path */

/*---------------------------------------------------------------------------
    Begin main loop through characters in filename.
  ---------------------------------------------------------------------------*/

    while ((workch = (uch)*cp++) != 0) {

        if (_ismbblead((unsigned char)workch)) {
            if (*cp) {
                *pp++ = (char)workch;
                *pp++ = (char)*cp++;
            }
            else
                *pp++ = '_';
            continue;
        }

        switch (workch) {
            case '/':             /* can assume -j flag not given */
                *pp = '\0';
                map2fat(pathcomp, last_dot);   /* 18.3 trunc. (in place) */
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

            /* drive names are not stored in zipfile, so no colons allowed;
             *  no brackets or most other punctuation either (all of which
             *  can appear in Unix-created archives; backslash is particularly
             *  bad unless all necessary directories exist) */

            case '[':          /* these punctuation characters forbidden */
            case ']':          /*  only on plain FAT file systems */
            case '+':
            case ',':
            case '=':
            case '<':
            case '>':
            case '|':
            case '\"':
            case '\'':
                if (!special_char)
                    workch = '_';
                *pp++ = (char)workch;
                break;

            case '-':
                if (pp == pathcomp && !special_char)
                    workch = '_';
                *pp++ = (char)workch;
                break;

            case ':':
            case '\\':
            case '*':
            case '?':
                *pp++ = '_';
                break;

            case ';':             /* VMS version (or DEC-20 attrib?) */
                lastsemi = pp;
                if (!special_char)
                    workch = '_';
                *pp++ = (char)workch;  /* keep for now; remove VMS ";##" */
                break;                 /*  later, if requested */

            case ' ':                      /* change spaces to underscores */
#if 0  /* do it always */
                if (uO.sflag)              /*  only if requested */
#endif
                    workch = '_';
                *pp++ = (char)workch;
                break;

            default:
                /* allow European characters in filenames: */
                if (isprint(workch) || workch >= 128)
                    *pp++ = (char)workch;

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

    if (G.filename[strlen(G.filename) - 1] == '/') {
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

    *pp = '\0';                   /* done with pathcomp:  terminate it */

    /* if not saving them, remove VMS version numbers (appended ";###") */
    if (!uO.V_flag && lastsemi) {
        pp = lastsemi + 1;
        while (isdigit((uch)(*pp)))
            ++pp;
        if (*pp == '\0')          /* only digits between ';' and end:  nuke */
            *lastsemi = '\0';
    }

    map2fat(pathcomp, last_dot);  /* 18.3 truncation (in place) */

    if (*pathcomp == '\0') {
        Info(slide, 1, ((char *)slide, "mapname:  conversion of %s failed\n",
          FnFilter1(G.filename)));
        return (error & ~MPN_MASK) | MPN_ERR_SKIP;
    }

    checkdir(__G__ pathcomp, APPEND_NAME);  /* returns 1 if truncated: care? */
    checkdir(__G__ G.filename, GETPATH);

    if (G.pInfo->vollabel) {    /* set the volume label now */
        int fd;

        if (QCOND2)
            Info(slide, 0, ((char *)slide, "  labelling: %s\n",
              FnFilter1(G.filename)));
        if ((fd = _dos_newfile(G.filename, G.pInfo->file_attr)) < 0) {
            Info(slide, 1, ((char *)slide,
              "mapname:  error setting volume label\n"));
            return (error & ~MPN_MASK) | MPN_ERR_SKIP;
        }
        _dos_close(fd);
        /* success:  skip the "extraction" quietly */
        return (error & ~MPN_MASK) | MPN_INF_SKIP;
    }

    return error;

} /* end function mapname() */




/**********************/
/* Function map2fat() */
/**********************/

static void map2fat(pathcomp, last_dot)
    char *pathcomp, *last_dot;
{
    char *np;

    if (pathcomp == last_dot) {         /* dotfile(e.g. ".foo") */
        pathcomp = last_dot;
        last_dot = (char *)NULL;
    }

    if (multi_period) {
        if (strlen(pathcomp) <= 18)
            return;
    }
    else {
        char *p;

        for (p = pathcomp; *p; p++)
            if (*p == (char)'.' && p != last_dot)
                *p = '_';
    }

    if (last_dot) {
        *last_dot++ = '\0';
        trunc_name(last_dot, 3);
    }
    np = trunc_name(pathcomp, 18);
    if (last_dot) {
        *--last_dot = '.';
        if (np)
            strcpy(np, last_dot);
    }

} /* end function map2fat() */

static char *trunc_name(char *name, int maxlen)
{

    if (strlen(name) <= maxlen)
        return (char *)NULL;

    do {
        if (_ismbblead((unsigned char)*name)) {
            if (--maxlen == 0)
                break;
            name++;
        }
        name++;
        maxlen--;
    } while (maxlen > 0);
    *name = '\0';

    return name;
}




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

        Trace((stderr, "appending dir segment [%s]\n", FnFilter1(pathcomp)));
        while ((*end = *pathcomp++) != '\0')
            ++end;

        /* GRR:  could do better check, see if overrunning buffer as we go:
         * check end-buildpath after each append, set warning variable if
         * within 20 of FILNAMSIZ; then if var set, do careful check when
         * appending.  Clear variable when begin new path. */

        if ((end-buildpath) > FILNAMSIZ-3)  /* need '/', one-char name, '\0' */
            too_long = TRUE;                /* check if extracting directory? */
        if (SSTAT(buildpath, &G.statbuf))   /* path doesn't exist */
        {
            if (!G.create_dirs) { /* told not to create (freshening) */
                free(buildpath);
                return MPN_INF_SKIP;    /* path doesn't exist: nothing to do */
            }
            if (too_long) {
                Info(slide, 1, ((char *)slide,
                  "checkdir error:  path too long: %s\n",
                  FnFilter1(buildpath)));
                free(buildpath);
                /* no room for filenames:  fatal */
                return MPN_ERR_TOOLONG;
            }
            if (mkdir(buildpath, 0777) == -1) {   /* create the directory */
                Info(slide, 1, ((char *)slide,
                  "checkdir error:  cannot create %s\n\
                 unable to process %s.\n",
                  FnFilter2(buildpath), FnFilter1(G.filename)));
                free(buildpath);
                /* path didn't exist, tried to create, failed */
                return MPN_ERR_SKIP;
            }
            created_dir = TRUE;
        } else if (!S_ISDIR(G.statbuf.st_mode)) {
            Info(slide, 1, ((char *)slide,
              "checkdir error:  %s exists but is not directory\n\
                 unable to process %s.\n",
              FnFilter2(buildpath), FnFilter1(G.filename)));
            free(buildpath);
            /* path existed but wasn't dir */
            return MPN_ERR_SKIP;
        }
        if (too_long) {
            Info(slide, 1, ((char *)slide,
              "checkdir error:  path too long: %s\n", FnFilter1(buildpath)));
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

        Trace((stderr, "appending filename [%s]\n", FnFilter1(pathcomp)));
        while ((*end = *pathcomp++) != '\0') {
            ++end;
            if ((end-buildpath) >= FILNAMSIZ) {
                *--end = '\0';
                Info(slide, 1, ((char *)slide,
                  "checkdir warning:  path too long; truncating\n\
                   %s\n                -> %s\n",
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
        if ((rootlen > 0) && !renamed_fullpath) {
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
    command line.
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
            } else if (rootlen > 0 && (SSTAT(tmproot, &G.statbuf) ||
                       !S_ISDIR(G.statbuf.st_mode))) /* path does not exist */
            {
                if (!G.create_dirs /* || iswild(tmproot) */ ) {
                    free(tmproot);
                    rootlen = 0;
                    /* skip (or treat as stored file) */
                    return MPN_INF_SKIP;
                }
                /* create the directory (could add loop here scanning tmproot
                 * to create more than one level, but why really necessary?) */
                if (mkdir(tmproot, 0777) == -1) {
                    Info(slide, 1, ((char *)slide,
                      "checkdir:  cannot create extraction directory: %s\n",
                      FnFilter1(tmproot)));
                    free(tmproot);
                    rootlen = 0;
                    /* path didn't exist, tried to create, and failed: */
                    /* file exists, or 2+ subdir levels required */
                    return MPN_ERR_SKIP;
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




#if (defined(USE_EF_UT_TIME) || defined(TIMESTAMP))
/* The following DOS date/time structure is machine-dependent as it
 * assumes "little-endian" byte order.  For MSDOS-specific code, which
 * is run on ix86 CPUs (or emulators), this assumption is valid; but
 * care should be taken when using this code as template for other ports.
 */
typedef union {
    ulg z_dostime;
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
{
    /* skip restoring time stamps on user's request */
    if (uO.D_flag <= 1) {
#ifdef USE_EF_UT_TIME
        dos_fdatetime dos_dt;
        iztimes z_utime;
        struct tm *t;
#endif /* USE_EF_UT_TIME */


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
        _dos_filedate(fileno(G.outfile), dos_dt.z_dostime);
#else /* !USE_EF_UT_TIME */
        _dos_filedate(fileno(G.outfile), G.lrec.last_mod_dos_datetime);
#endif /* ?USE_EF_UT_TIME */
    }

    fclose(G.outfile);

    _dos_chmod(G.filename, G.pInfo->file_attr);

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
        (_dos_filedate(fd, dos_dt.z_dostime)))
    {
        if (fd != -1)
            close(fd);
        return -1;
    }
    close(fd);
    return 0;

} /* end function stamp_file() */

#endif /* TIMESTAMP */




#ifndef SFX

/************************/
/*  Function version()  */
/************************/

void version(__G)
    __GDEF
{
    int len;
#if 0
    char buf[40];
#endif

    len = sprintf((char *)slide, LoadFarString(CompiledWith),

#ifdef __GNUC__
      "gcc ", __VERSION__,
#else
#  if 0
      "cc ", (sprintf(buf, " version %d", _RELEASE), buf),
#  else
      "unknown compiler", "",
#  endif
#endif

      "Human68k",
#ifdef __MC68020__
      " (X68030)",
#else
      " (X680x0)",
#endif

#ifdef __DATE__
      " on ", __DATE__
#else
      "", ""
#endif
      );

    (*G.message)((zvoid *)&G, slide, (ulg)len, 0);

} /* end function version() */

#endif /* !SFX */


#if defined (SFX) && defined (MAIN)
int main(int argc, char *argv[])
{
    char argv0[92];

    /* make true argv[0] (startup routine makes it inaccuracy) */
    argv[0] = strcat (strcpy (argv0, _procp->exe_path), _procp->exe_name);

    return MAIN(argc, argv);
}
#endif /* SFX && MAIN */
