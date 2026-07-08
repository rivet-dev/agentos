/*
  Copyright (c) 1990-2007 Info-ZIP.  All rights reserved.

  See the accompanying file LICENSE, version 2000-Apr-09 or later
  (the contents of which are also included in unzip.h) for terms of use.
  If, for some reason, all these files are missing, the Info-ZIP license
  also may be found at:  ftp://ftp.info-zip.org/pub/infozip/license.html
*/
/*---------------------------------------------------------------------------

  flexos.c

  FlexOS-specific routines for use with Info-ZIP's UnZip 5.2 and later.

  Based upon the MSDOS version of this file (msdos/msdos.c)

  Contains:  do_wild()
             mapattr()
             mapname()
             map2fat()
             checkdir()
             close_outfile()
             dateformat()
             version()
             _wildarg()

  ---------------------------------------------------------------------------*/



#define UNZIP_INTERNAL
#include "unzip.h"

#include <flexif.h>

/* The following should really be a static declaration,  but the compiler
   complains (crappy compiler can't cope with a static forward declaration).
 */
extern void map2fat OF((char *pathcomp, char *last_dot));

static int created_dir;        /* used by mapname(), checkdir() */
static int renamed_fullpath;   /* ditto */

/*****************************/
/*  Strings used in flexos.c  */
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

#include <dirent.h>

#ifndef SFX

/************************/
/*  Function do_wild()  */   /* identical to OS/2 version */
/************************/

char *do_wild(__G__ wildspec)
    __GDEF
    ZCONST char *wildspec;   /* only used first time on a given dir */
{
    static DIR *wild_dir = (DIR *)NULL;
    static ZCONST char *wildname;
    static char *dirname, matchname[FILNAMSIZ];
    static int notfirstcall=FALSE, have_dirname, dirnamelen;
    char *fnamestart;
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
            dir = NULL;
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
/* GRR:  cannot strip trailing char for opendir since might be "d:/" or "d:"
 *       (would have to check for "./" at end--let opendir handle it instead) */
            strncpy(dirname, wildspec, dirnamelen);
            dirname[dirnamelen] = '\0';   /* terminate for strcpy below */
            have_dirname = TRUE;
        }
        Trace((stderr, "do_wild:  dirname = [%s]\n", FnFilter1(dirname)));

        if ((wild_dir = opendir(dirname)) != (DIR *)NULL) {
            if (have_dirname) {
                strcpy(matchname, dirname);
                fnamestart = matchname + dirnamelen;
            } else
                fnamestart = matchname;
            while ((file = readdir(wild_dir)) != (struct dirent *)NULL) {
                Trace((stderr, "do_wild:  readdir returns %s\n",
                  FnFilter1(file->d_name)));
                strcpy(fnamestart, file->d_name);
                if (strrchr(fnamestart, '.') == (char *)NULL)
                    strcat(fnamestart, ".");
                if (match(fnamestart, wildname, 1 WISEP) && /* 1=ignore case */
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
            closedir(wild_dir);
            wild_dir = (DIR *)NULL;
        }
#ifdef DEBUG
        else {
            Trace((stderr, "do_wild:  opendir(%s) returns NULL\n",
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
    if (have_dirname) {
        /* strcpy(matchname, dirname); */
        fnamestart = matchname + dirnamelen;
    } else
        fnamestart = matchname;
    while ((file = readdir(wild_dir)) != (struct dirent *)NULL) {
        Trace((stderr, "do_wild:  readdir returns %s\n",
          FnFilter1(file->d_name)));
        strcpy(fnamestart, file->d_name);
        if (strrchr(fnamestart, '.') == (char *)NULL)
            strcat(fnamestart, ".");
        if (match(fnamestart, wildname, 1 WISEP)) {     /* 1 == ignore case */
            Trace((stderr, "do_wild:  match() succeeds\n"));
            /* remove trailing dot */
            fnamestart += strlen(fnamestart) - 1;
            if (*fnamestart == '.')
                *fnamestart = '\0';
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
    /* set archive bit (file is not backed up): */
    G.pInfo->file_attr = (unsigned)(G.crec.external_file_attributes & 7) | 32;
    return 0;
}



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
    char *last_dot=(char *)NULL;   /* last dot not converted to underscore */
    int dotname = FALSE;           /* path component begins with dot? */
    int killed_ddot = FALSE;       /* is set when skipping "../" pathcomp */
    int error = MPN_OK;
    register unsigned workch;      /* hold the character being tested */


    if (G.pInfo->vollabel)
        return MPN_VOL_LABEL;   /* Cannot set disk volume labels in FlexOS */

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
                map2fat(pathcomp, last_dot);   /* 8.3 truncation (in place) */
                last_dot = (char *)NULL;
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

            case '.':
                if (pp == pathcomp) {     /* nothing appended yet... */
                    if (*cp == '.' && cp[1] == '/') {   /* "../" */
                        *pp++ = '.';      /* add first dot, unchanged... */
                        ++cp;             /* skip second dot, since it will */
                    } else {              /*  be "added" at end of if-block */
                        *pp++ = '_';      /* FAT doesn't allow null filename */
                        dotname = TRUE;   /*  bodies, so map .exrc -> _.exrc */
                    }                     /*  (extra '_' now, "dot" below) */
                } else if (dotname) {     /* found a second dot, but still */
                    dotname = FALSE;      /*  have extra leading underscore: */
                    *pp = '\0';           /*  remove it by shifting chars */
                    pp = pathcomp + 1;    /*  left one space (e.g., .p1.p2: */
                    while (pp[1]) {       /*  __p1 -> _p1_p2 -> _p1.p2 when */
                        *pp = pp[1];      /*  finished) [opt.:  since first */
                        ++pp;             /*  two chars are same, can start */
                    }                     /*  shifting at second position] */
                }
                last_dot = pp;    /* point at last dot so far... */
                *pp++ = '_';      /* convert dot to underscore for now */
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
                break;

            case ' ':                      /* change spaces to underscores */
                if (uO.sflag)              /*  only if requested */
                    *pp++ = '_';
                else
                    *pp++ = (char)workch;
                break;

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
            /* set dir time (note trailing '/') */
            return (error & ~MPN_MASK) | MPN_CREATED_DIR;
        }
        /* dir existed already; don't look for data to extract */
        return (error & ~MPN_MASK) | MPN_INF_SKIP;
    }

    *pp = '\0';                   /* done with pathcomp:  terminate it */

    /* if not saving them, remove VMS version numbers (appended ";###") */
    if (!uO.V_flag && lastsemi) {
        pp = lastsemi;            /* semi-colon was omitted:  expect all #'s */
        while (isdigit((uch)(*pp)))
            ++pp;
        if (*pp == '\0')          /* only digits between ';' and end:  nuke */
            *lastsemi = '\0';
    }

    map2fat(pathcomp, last_dot);  /* 8.3 truncation (in place) */

    if (*pathcomp == '\0') {
        Info(slide, 1, ((char *)slide, LoadFarString(ConversionFailed),
          FnFilter1(G.filename)));
        return (error & ~MPN_MASK) | MPN_ERR_SKIP;
    }

    checkdir(__G__ pathcomp, APPEND_NAME);  /* returns 1 if truncated: care? */
    checkdir(__G__ G.filename, GETPATH);

    return error;

} /* end function mapname() */




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

    /* pEnd = pathcomp + strlen(pathcomp); */
    if (last_dot == (char *)NULL) {   /* no dots:  check for underscores... */
        char *plu = strrchr(pathcomp, '_');   /* pointer to last underscore */

        if (plu == (char *)NULL) {  /* no dots, no underscores:  truncate at */
            if (pEnd > pathcomp+8)  /* 8 chars (could insert '.' and keep 11) */
                *(pEnd = pathcomp+8) = '\0';
        } else if (MIN(plu - pathcomp, 8) + MIN(pEnd - plu - 1, 3) > 8) {
            last_dot = plu;       /* be lazy:  drop through to next if-block */
        } else if ((pEnd - pathcomp) > 8)    /* more fits into just basename */
            pathcomp[8] = '\0';    /* than if convert last underscore to dot */
        /* else whole thing fits into 8 chars or less:  no change */
    }

/*---------------------------------------------------------------------------
    Case 2:  filename has dot in it, so truncate first half at 8 chars (shift
    extension if necessary) and second half at three.
  ---------------------------------------------------------------------------*/

    if (last_dot != (char *)NULL) {   /* one dot (or two, in the case of */
        *last_dot = '.';              /*  "..") is OK:  put it back in */

        if ((last_dot - pathcomp) > 8) {
            char *p=last_dot, *q=pathcomp+8;
            int i;

            for (i = 0;  (i < 4) && *p;  ++i)  /* too many chars in basename: */
                *q++ = *p++;                   /*  shift extension left and */
            *q = '\0';                         /*  truncate/terminate it */
        } else if ((pEnd - last_dot) > 4)
            last_dot[4] = '\0';                /* too many chars in extension */
        /* else filename is fine as is:  no change */
    }
} /* end function map2fat() */




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
        if (stat(buildpath, &G.statbuf))    /* path doesn't exist */
        {
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
            if (mkdir(buildpath, 0777) == -1) {   /* create the directory */
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
        if (renamed_fullpath) {   /* pathcomp = valid data */
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
    command line.  Note that under FlexOS, if a candidate extract-to
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
            int had_trailing_pathsep=FALSE, add_dot=FALSE;
            char *tmproot;

            if ((tmproot = (char *)malloc(rootlen+3)) == (char *)NULL) {
                rootlen = 0;
                return MPN_NOMEM;
            }
            strcpy(tmproot, pathcomp);
            if (tmproot[rootlen-1] == '/' || tmproot[rootlen-1] == '\\') {
                tmproot[--rootlen] = '\0';
                had_trailing_pathsep = TRUE;
            }
            if (tmproot[rootlen-1] == ':') {
                if (!had_trailing_pathsep)  /* i.e., original wasn't "xxx:/" */
                    add_dot = TRUE;     /* relative path: add '.' before '/' */
            } else if (rootlen > 0) &&  /* need not check "xxx:." and "xxx:/" */
                       (SSTAT(tmproot, &G.statbuf) ||
                        !S_ISDIR(G.statbuf.st_mode))
            {
                /* path does not exist */
                if (!G.create_dirs /* || iswild(tmproot) */ ) {
                    free(tmproot);
                    rootlen = 0;
                    /* treat as stored file */
                    return MPN_INF_SKIP;
                }
/* GRR:  scan for wildcard characters?  OS-dependent...
 * if find any, return MPN_INF_SKIP: treat as stored file(s) */
                /* create directory (could add loop here scanning tmproot
                 * to create more than one level, but really necessary?) */
                if (mkdir(tmproot, 0777) == -1) {
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




/****************************/
/* Function close_outfile() */
/****************************/

void close_outfile(__G)
    __GDEF
 /*
  * FlexOS VERSION
  *
  * Set the output file date/time stamp according to information from the
  * zipfile directory record for this member, then close the file and set
  * its permissions (archive, hidden, read-only, system).  Aside from closing
  * the file, this routine is optional (but most compilers support it).
  */
{
    DISKFILE    df;
    LONG        fnum;

    struct {                /* date and time words */
        union {             /* DOS file modification time word */
            ush ztime;
            struct {
                unsigned zt_se : 5;
                unsigned zt_mi : 6;
                unsigned zt_hr : 5;
            } _tf;
        } _t;
        union {             /* DOS file modification date word */
            ush zdate;
            struct {
                unsigned zd_dy : 5;
                unsigned zd_mo : 4;
                unsigned zd_yr : 7;
            } _df;
        } _d;
    } zt;

#ifdef USE_EF_UT_TIME
    iztimes z_utime;
    struct tm *t;
#endif /* ?USE_EF_UT_TIME */

    fclose(G.outfile);

    if ((fnum = s_open(A_SET, G.filename)) < 0) {
        Info(slide, 0x201, ((char *)slide,
          "warning:  cannot open %s to set the time\n",
          FnFilter1(G.filename)));
        return;
    }

    if (s_get(T_FILE, fnum, &df, DSKFSIZE) < 0) {
        s_close(0, fnum);

        Info(slide, 0x201, ((char *)slide,
          "warning:  cannot get info on %s\n", FnFilter1(G.filename)));
        return;
    }

    /* skip restoring time stamps on user's request */
    if (uO.D_flag <= 1) {

/*---------------------------------------------------------------------------
    Copy and/or convert time and date variables, if necessary; then fill in
    the file time/date.
  ---------------------------------------------------------------------------*/

#ifdef USE_EF_UT_TIME
        if (G.extra_field &&
#ifdef IZ_CHECK_TZ
            G.tz_is_valid &&
#endif
            (ef_scan_for_izux(G.extra_field, G.lrec.extra_field_length, 0,
             G.lrec.last_mod_dos_datetime, &z_utime, NULL) & EB_UT_FL_MTIME))
        {
            TTrace((stderr, "close_outfile:  Unix e.f. modif. time = %ld\n",
              z_utime.mtime));
            t = localtime(&(z_utime.mtime));
        } else
            t = (struct tm *)NULL;
        if (t != (struct tm *)NULL) {
            if (t->tm_year < 80) {
                df.df_modyear = 1980;
                df.df_modmonth = 1;
                df.df_modday = 1;
                df.df_modhr = 0;
                df.df_modmin = 0;
                df.df_modsec = 0;
            } else {
                df.df_modyear = t->tm_year + 1900;
                df.df_modmonth = t->tm_mon + 1;
                df.df_modday = t->tm_mday;
                df.df_modhr = t->tm_hour;
                df.df_modmin = t->tm_min;
                df.df_modsec = t->tm_sec;
            }
        } else
#endif /* ?USE_EF_UX_TIME */
        {
            zt._t.ztime = (ush)(G.lrec.last_mod_dos_datetime) & 0xffff;
            zt._d.zdate = (ush)(G.lrec.last_mod_dos_datetime >> 16);

            df.df_modyear = 1980 + zt._d._df.zd_yr;
            df.df_modmonth = zt._d._df.zd_mo;
            df.df_modday = zt._d._df.zd_dy;
            df.df_modhr = zt._t._tf.zt_hr;
            df.df_modmin = zt._t._tf.zt_mi;
            df.df_modsec = zt._t._tf.zt_se << 1;
        }
    }

/*---------------------------------------------------------------------------
    Fill in the file attributes.
  ---------------------------------------------------------------------------*/

    df.df_attr1 = (UBYTE)G.pInfo->file_attr;

/*---------------------------------------------------------------------------
    Now we try to set the attributes & date/time.
  ---------------------------------------------------------------------------*/

    if (s_set(T_FILE, fnum, &df, DSKFSIZE) < 0)
        Info(slide, 0x201, ((char *)slide,
          "warning:  cannot set info for %s\n", FnFilter1(G.filename)));

    s_close(0, fnum);
} /* end function close_outfile() */

#ifndef SFX

/*************************/
/* Function dateformat() */
/*************************/

int dateformat()
{
    return DF_DMY;   /* default for systems without locale info */
}

/************************/
/*  Function version()  */
/************************/

void version(__G)
    __GDEF
{
    int len;

    len = sprintf((char *)slide, LoadFarString(CompiledWith),
            "MetaWare High C",
            "",
            "FlexOS",
            " (16-bit, big)",

#ifdef __DATE__
      " on ", __DATE__
#else
      "", ""
#endif
    );

    (*G.message)((zvoid *)&G, slide, (ulg)len, 0);
}

#endif /* !SFX */

/************************/
/*  Function _wildarg() */
/************************/

/* This prevents the PORTLIB startup code from preforming argument globbing */

_wildarg() {}
