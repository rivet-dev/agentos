/*
  Copyright (c) 1990-2007 Info-ZIP.  All rights reserved.

  See the accompanying file LICENSE, version 2000-Apr-09 or later
  (the contents of which are also included in unzip.h) for terms of use.
  If, for some reason, all these files are missing, the Info-ZIP license
  also may be found at:  ftp://ftp.info-zip.org/pub/infozip/license.html
*/
/*------------------------------------------------------------------------

  amiga.c

  Amiga-specific routines for use with Info-ZIP's UnZip 5.1 and later.
  See History.5xx for revision history.

  Contents:   do_wild()
              mapattr()
              mapname()
              checkdir()
              close_outfile()
              stamp_file()
              _abort()                (Aztec C only)
             [dateformat()]           (currently not used)
              screensize()
              version()

  ------------------------------------------------------------------------*/


#define UNZIP_INTERNAL
#ifdef AZTEC_C
#  define NO_FCNTL_H
#endif
#include "unzip.h"
#include "unzvers.h"

/* Globular varibundus -- now declared in SYSTEM_SPECIFIC_GLOBALS in amiga.h */

/* static int created_dir; */      /* used in mapname(), checkdir() */
/* static int renamed_fullpath; */ /* ditto */

#define PERMS   0777
#define MKDIR(path,mode) mkdir(path)

#ifndef S_ISCRIPT          /* not having one implies you have none */
#  define S_IARCHIVE 0020  /* not modified since this bit was last set */
#  define S_IREAD    0010  /* can be opened for reading */
#  define S_IWRITE   0004  /* can be opened for writing */
#  define S_IDELETE  0001  /* can be deleted */
#endif /* S_ISCRIPT */

#ifndef S_IRWD
#  define S_IRWD     0015  /* useful combo of Amiga privileges */
#endif /* !S_IRWD */

#ifndef S_IHIDDEN
#  define S_IHIDDEN  0200  /* hidden supported in future AmigaDOS (someday) */
#endif /* !S_HIDDEN */

#ifndef SFX
/* Make sure the number here matches unzvers.h in the *EXACT* form */
/* UZ_MAJORVER "." UZ_MINORVER UZ_PATCHLEVEL vvvv  No non-digits!  */
const char version_id[]  = "\0$VER: UnZip " UZ_VER_STRING " ("
#include "env:VersionDate"
   ")\r\n";
#endif /* SFX */


static int ispattern(ZCONST char *p)
{
    register char c;
    while (c = *p++)
        if (c == '\\') {
            if (!*++p)
                return FALSE;
        } else if (c == '?' || c == '*')
            return TRUE;
        else if (c == '[') {
            for (;;) {
                if (!(c = *p++))
                    return FALSE;
                else if (c == '\\') {
                    if (!*++p)
                        return FALSE;
                } else if (c == ']')
                    return TRUE;
            }
        }
    return FALSE;
}

/**********************/
/* Function do_wild() */
/**********************/

char *do_wild(__G__ wildspec)
    __GDEF
    ZCONST char *wildspec;  /* only used first time on a given dir */
{
/* these statics are now declared in SYSTEM_SPECIFIC_GLOBALS in amiga.h:
    static DIR *wild_dir = NULL;
    static ZCONST char *wildname;
    static char *dirname, matchname[FILNAMSIZ];
    static int notfirstcall = FALSE, dirnamelen;
*/
    struct dirent *file;
    BPTR lok = 0;

    /* Even when we're just returning wildspec, we *always* do so in
     * matchname[]--calling routine is allowed to append four characters
     * to the returned string, and wildspec may be a pointer to argv[].
     */
    if (!G.notfirstcall) {      /* first call:  must initialize everything */
        G.notfirstcall = TRUE;

        /* avoid needless readdir() scans: */
        if (!ispattern(wildspec) ||
                (lok = Lock((char *)wildspec, ACCESS_READ))) {
            if (lok) UnLock(lok);       /* ^^ we ignore wildcard chars if */
            G.dirnamelen = 0;           /* the name matches a real file   */
            strncpy(G.matchname, wildspec, FILNAMSIZ);
            G.matchname[FILNAMSIZ-1] = '\0';
            return G.matchname;
        }

        /* break the wildspec into a directory part and a wildcard filename */
        if ((G.wildname = (ZCONST char *)strrchr(wildspec, '/')) == NULL &&
            (G.wildname = (ZCONST char *)strrchr(wildspec, ':')) == NULL) {
            G.dirname = "";             /* current dir */
            G.dirnamelen = 0;
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
            G.dirname[G.dirnamelen] = '\0';
        }

        if ((G.wild_dir = opendir(G.dirname)) != NULL) {
            while ((file = readdir(G.wild_dir)) != NULL) {
                if (match(file->d_name, G.wildname, 1 WISEP)) {/* ignore case */
                    strcpy(G.matchname, G.dirname);
                    strcpy(G.matchname + G.dirnamelen, file->d_name);
                    return G.matchname;
                }
            }
            /* if we get to here directory is exhausted, so close it */
            closedir(G.wild_dir);
            G.wild_dir = NULL;
        }

        /* return the raw wildspec in case that works (e.g., directory not
         * searchable, but filespec was not wild and file is readable) */
        strncpy(G.matchname, wildspec, FILNAMSIZ);
        G.matchname[FILNAMSIZ-1] = '\0';
        return G.matchname;
    }

    /* last time through, might have failed opendir but returned raw wildspec */
    if (G.wild_dir == NULL) {
        G.notfirstcall = FALSE;    /* nothing left to try -- reset */
        if (G.dirnamelen > 0)
            free(G.dirname);
        return (char *)NULL;
    }

    /* If we've gotten this far, we've read and matched at least one entry
     * successfully (in a previous call), so dirname has been copied into
     * matchname already.
     */
    while ((file = readdir(G.wild_dir)) != NULL)
        if (match(file->d_name, G.wildname, 1 WISEP)) { /* 1 == ignore case */
            /* strcpy(G.matchname, dirname); */
            strcpy(G.matchname + G.dirnamelen, file->d_name);
            return G.matchname;
        }

    closedir(G.wild_dir);  /* have read at least one dir entry; nothing left */
    G.wild_dir = NULL;
    G.notfirstcall = FALSE; /* reset for new wildspec */
    if (G.dirnamelen > 0)
        free(G.dirname);
    return (char *)NULL;

} /* end function do_wild() */




/**********************/
/* Function mapattr() */
/**********************/

int mapattr(__G)      /* Amiga version */
    __GDEF
{
    ulg  tmp = G.crec.external_file_attributes;


    /* Amiga attributes = hsparwed = hidden, script, pure, archive,
     * read, write, execute, delete */

    switch (G.pInfo->hostnum) {
        case AMIGA_:
            if ((tmp & 1) == (tmp>>18 & 1))
                tmp ^= 0x000F0000;      /* PKAZip compatibility kluge */
            /* turn off archive bit for restored Amiga files */
            G.pInfo->file_attr = (unsigned)((tmp>>16) & (~S_IARCHIVE));
            break;

        case UNIX_:   /* preserve read, write, execute:  use logical-OR of */
        case VMS_:    /* user, group, and other; if writable, set delete bit */
        case ACORN_:
        case ATARI_:
        case ATHEOS_:
        case BEOS_:
        case QDOS_:
        case TANDEM_:
            {
              unsigned uxattr = (unsigned)(tmp >> 16);
              int r = FALSE;

              if (uxattr == 0 && G.extra_field) {
                /* Some (non-Info-ZIP) implementations of Zip for Unix and
                   VMS (and probably others ??) leave 0 in the upper 16-bit
                   part of the external_file_attributes field. Instead, they
                   store file permission attributes in some extra field.
                   As a work-around, we search for the presence of one of
                   these extra fields and fall back to the MSDOS compatible
                   part of external_file_attributes if one of the known
                   e.f. types has been detected.
                   Later, we might implement extraction of the permission
                   bits from the VMS extra field. But for now, the work-around
                   should be sufficient to provide "readable" extracted files.
                   (For ASI Unix e.f., an experimental remap of the e.f.
                   mode value IS already provided!)
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
                uxattr = (( uxattr>>6 | uxattr>>3 | uxattr) & 07) << 1;
                G.pInfo->file_attr = (unsigned)(uxattr&S_IWRITE ?
                                                uxattr|S_IDELETE : uxattr);
                break;
              }
            }
            /* fall through! */

        /* all other platforms:  assume read-only bit in DOS half of attribute
         * word is set correctly ==> will become READ or READ+WRITE+DELETE */
        case FS_FAT_:
        case FS_HPFS_:  /* can add S_IHIDDEN check to MSDOS/OS2/NT eventually */
        case FS_NTFS_:
        case MAC_:
        case TOPS20_:
        default:
            G.pInfo->file_attr = (unsigned)(tmp&1? S_IREAD : S_IRWD);
            break;

    } /* end switch (host-OS-created-by) */

    G.pInfo->file_attr &= 0xff;   /* mask off all but lower eight bits */
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
    int killed_ddot = FALSE;    /* is set when skipping "../" pathcomp */
    int error = MPN_OK;
    register unsigned workch;   /* hold the character being tested */


/*---------------------------------------------------------------------------
    Initialize various pointers and counters and stuff.
  ---------------------------------------------------------------------------*/

    if (G.pInfo->vollabel)
        return MPN_VOL_LABEL;   /* can't set disk volume labels in AmigaDOS */

    /* can create path as long as not just freshening, or if user told us */
    G.create_dirs = (!uO.fflag || renamed);

    G.created_dir = FALSE;      /* not yet */

    /* user gave full pathname:  don't prepend G.rootpath */
#ifndef OLD_AMIGA_RENAMED
    G.renamed_fullpath = (renamed &&
                          (*G.filename == '/' || *G.filename == ':'));
#else
    /* supress G.rootpath even when user gave a relative pathname */
# if 1
    G.renamed_fullpath = (renamed && strpbrk(G.filename, ":/");
# else
    G.renamed_fullpath = (renamed &&
                          (strchr(G.filename, ':') || strchr(G.filename, '/')));
# endif
#endif

    if (checkdir(__G__ (char *)NULL, INIT) == MPN_NOMEM)
        return MPN_NOMEM;       /* initialize path buffer, unless no memory */

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

            case ';':             /* VMS version (or DEC-20 attrib?) */
                lastsemi = pp;         /* keep for now; remove VMS ";##" */
                *pp++ = (char)workch;  /*  later, if requested */
                break;

            default:
                /* allow ISO European characters in filenames: */
                if (isprint(workch) || (160 <= workch && workch <= 255))
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
        if (G.created_dir) {
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

    if (*pathcomp == '\0') {
        Info(slide, 1, ((char *)slide, "mapname:  conversion of %s failed\n",
          FnFilter1(G.filename)));
        return (error & ~MPN_MASK) | MPN_ERR_SKIP;
    }

    error = (error & ~MPN_MASK) | checkdir(__G__ pathcomp, APPEND_NAME);
    if ((error & MPN_MASK) == MPN_INF_TRUNC) {
        /* GRR:  OK if truncated here:  warn and continue */
        /* (warn in checkdir?) */
    }
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
/* these statics are now declared in SYSTEM_SPECIFIC_GLOBALS in amiga.h: */
/*  static int rootlen = 0; */   /* length of rootpath */
/*  static char *rootpath;  */   /* user's "extract-to" directory */
/*  static char *buildpath; */   /* full path (so far) to extracted file */
/*  static char *end;       */   /* pointer to end of buildpath ('\0') */

#   define FN_MASK   7
#   define FUNCTION  (flag & FN_MASK)


/*---------------------------------------------------------------------------
    APPEND_DIR:  append the path component to the path being built and check
    for its existence.  If doesn't exist and we are creating directories, do
    so for this one; else signal success or error as appropriate.
  ---------------------------------------------------------------------------*/

/* GRR:  check path length after each segment:  warn about truncation */

    if (FUNCTION == APPEND_DIR) {
        int too_long = FALSE;

        Trace((stderr, "appending dir segment [%s]\n", FnFilter1(pathcomp)));
        while ((*G.build_end = *pathcomp++) != '\0')
            ++G.build_end;
        /* Truncate components over 30 chars? Nah, the filesystem handles it. */
        if ((G.build_end-G.buildpath) > FILNAMSIZ-3)       /* room for "/a\0" */
            too_long = TRUE;                    /* check if extracting dir? */
        if (SSTAT(G.buildpath, &G.statbuf)) {   /* path doesn't exist */
            if (!G.create_dirs) { /* told not to create (freshening) */
                free(G.buildpath);
                return MPN_INF_SKIP;    /* path doesn't exist: nothing to do */
            }
            if (too_long) {
                Info(slide, 1, ((char *)slide,
                  "checkdir error:  path too long: %s\n",
                  FnFilter1(G.buildpath)));
                free(G.buildpath);
                /* no room for filenames:  fatal */
                return MPN_ERR_TOOLONG;
            }
            if (MKDIR(G.buildpath, 0777) == -1) {   /* create the directory */
                Info(slide, 1, ((char *)slide,
                  "checkdir error:  cannot create %s\n\
                 unable to process %s.\n",
                  FnFilter2(G.buildpath), FnFilter1(G.filename)));
                free(G.buildpath);
                /* path didn't exist, tried to create, failed */
                return MPN_ERR_SKIP;
            }
            G.created_dir = TRUE;
        } else if (!S_ISDIR(G.statbuf.st_mode)) {
            Info(slide, 1, ((char *)slide,
              "checkdir error:  %s exists but is not directory\n\
                 unable to process %s.\n",
              FnFilter2(G.buildpath), FnFilter1(G.filename)));
            free(G.buildpath);
            /* path existed but wasn't dir */
            return MPN_ERR_SKIP;
        }
        if (too_long) {
            Info(slide, 1, ((char *)slide,
              "checkdir error:  path too long: %s\n", FnFilter1(G.buildpath)));
            free(G.buildpath);
            /* no room for filenames:  fatal */
            return MPN_ERR_TOOLONG;
        }
        *G.build_end++ = '/';
        *G.build_end = '\0';
        Trace((stderr, "buildpath now = [%s]\n", FnFilter1(G.buildpath)));
        return MPN_OK;

    } /* end if (FUNCTION == APPEND_DIR) */

/*---------------------------------------------------------------------------
    GETPATH:  copy full path to the string pointed at by pathcomp, and free
    G.buildpath.  Not our responsibility to worry whether pathcomp has room.
  ---------------------------------------------------------------------------*/

    if (FUNCTION == GETPATH) {
        strcpy(pathcomp, G.buildpath);
        Trace((stderr, "getting and freeing path [%s]\n",
          FnFilter1(pathcomp)));
        free(G.buildpath);
        G.buildpath = G.build_end = (char *)NULL;
        return MPN_OK;
    }

/*---------------------------------------------------------------------------
    APPEND_NAME:  assume the path component is the filename; append it and
    return without checking for existence.
  ---------------------------------------------------------------------------*/

    if (FUNCTION == APPEND_NAME) {
        Trace((stderr, "appending filename [%s]\n", FnFilter1(pathcomp)));
        while ((*G.build_end = *pathcomp++) != '\0') {
            ++G.build_end;
            if ((G.build_end-G.buildpath) >= FILNAMSIZ) {
                *--G.build_end = '\0';
                Info(slide, 0x201, ((char *)slide,
                  "checkdir warning:  path too long; truncating\n\
                   %s\n                -> %s\n",
                  FnFilter1(G.filename), FnFilter2(G.buildpath)));
                return MPN_INF_TRUNC;   /* filename truncated */
            }
        }
        Trace((stderr, "buildpath now = [%s]\n", FnFilter1(G.buildpath)));
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
        if ((G.buildpath = (char *)malloc(strlen(G.filename)+G.rootlen+1))
            == (char *)NULL)
            return MPN_NOMEM;
        if ((G.rootlen > 0) && !G.renamed_fullpath) {
            strcpy(G.buildpath, G.rootpath);
            G.build_end = G.buildpath + G.rootlen;
        } else {
            *G.buildpath = '\0';
            G.build_end = G.buildpath;
        }
        Trace((stderr, "[%s]\n", FnFilter1(G.buildpath)));
        return MPN_OK;
    }

/*---------------------------------------------------------------------------
    ROOT:  if appropriate, store the path in G.rootpath and create it if
    necessary; else assume it's a zipfile member and return.  This path
    segment gets used in extracting all members from every zipfile specified
    on the command line.
  ---------------------------------------------------------------------------*/

#if (!defined(SFX) || defined(SFX_EXDIR))
    if (FUNCTION == ROOT) {
        Trace((stderr, "initializing root path to [%s]\n",
          FnFilter1(pathcomp)));
        if (pathcomp == (char *)NULL) {
            G.rootlen = 0;
            return MPN_OK;
        }
        if (G.rootlen > 0)      /* rootpath was already set, nothing to do */
            return MPN_OK;
        if ((G.rootlen = strlen(pathcomp)) > 0) {
            if (stat(pathcomp, &G.statbuf) || !S_ISDIR(G.statbuf.st_mode)) {
                /* path does not exist */
                if (!G.create_dirs) {
                    G.rootlen = 0;
                    /* skip (or treat as stored file) */
                    return MPN_INF_SKIP;
                }
                /* create the directory (could add loop here scanning pathcomp
                 * to create more than one level, but why really necessary?) */
                if (MKDIR(pathcomp, 0777) == -1) {
                    Info(slide, 1, ((char *)slide,
                      "checkdir:  cannot create extraction directory: %s\n",
                      FnFilter1(pathcomp)));
                    G.rootlen = 0;
                    /* path didn't exist, tried to create, and failed: */
                    /* file exists, or 2+ subdir levels required */
                    return MPN_ERR_SKIP;
                }
            }
            if ((G.rootpath = (char *)malloc(G.rootlen+2)) == NULL) {
                G.rootlen = 0;
                return MPN_NOMEM;
            }
            strcpy(G.rootpath, pathcomp);
            if (G.rootpath[G.rootlen-1] != ':' && G.rootpath[G.rootlen-1] != '/')
                G.rootpath[G.rootlen++] = '/';
            G.rootpath[G.rootlen] = '\0';
            Trace((stderr, "rootpath now = [%s]\n", FnFilter1(G.rootpath)));
        }
        return MPN_OK;
    }
#endif /* !SFX || SFX_EXDIR */

/*---------------------------------------------------------------------------
    END:  free G.rootpath, immediately prior to program exit.
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





/**************************************/
/* Function close_outfile() */
/**************************************/
/* this part differs slightly with Zip */
/*-------------------------------------*/

void close_outfile(__G)
    __GDEF
{
    time_t m_time;
#ifdef USE_EF_UT_TIME
    iztimes z_utime;
#endif
    LONG FileDate();

    if (uO.cflag)               /* can't set time or filenote on stdout */
        return;

  /* close the file *before* setting its time under AmigaDOS */

    fclose(G.outfile);

    /* skip restoring time stamps on user's request */
    if (uO.D_flag <= 1) {
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
            m_time = z_utime.mtime;
        } else {
            /* Convert DOS time to time_t format */
            m_time = dos_to_unix_time(G.lrec.last_mod_dos_datetime);
        }
#else /* !USE_EF_UT_TIME */
        /* Convert DOS time to time_t format */
        m_time = dos_to_unix_time(G.lrec.last_mod_dos_datetime);
#endif /* ?USE_EF_UT_TIME */

#ifdef DEBUG
        Info(slide, 1, ((char *)slide, "\nclose_outfile(): m_time=%s\n",
             ctime(&m_time)));
#endif

        if (!FileDate(G.filename, &m_time))
            Info(slide, 1, ((char *)slide,
                 "warning:  cannot set the time for %s\n", G.filename));
    }

    /* set file perms after closing (not done at creation)--see mapattr() */

    chmod(G.filename, G.pInfo->file_attr);

    /* give it a filenote from the zipfile comment, if appropriate */

    if (uO.N_flag && G.filenotes[G.filenote_slot]) {
        SetComment(G.filename, G.filenotes[G.filenote_slot]);
        free(G.filenotes[G.filenote_slot]);
        G.filenotes[G.filenote_slot] = NULL;
    }

} /* end function close_outfile() */


#ifdef TIMESTAMP

/*************************/
/* Function stamp_file() */
/*************************/

int stamp_file(fname, modtime)
    ZCONST char *fname;
    time_t modtime;
{
    time_t m_time;
    LONG FileDate();

    m_time = modtime;
    return (FileDate((char *)fname, &m_time));

} /* end function stamp_file() */

#endif /* TIMESTAMP */


#ifndef __SASC
/********************************************************************/
/* Load filedate as a separate external file; it's used by Zip, too.*/
/*                                                                  */
#  include "amiga/filedate.c"                                    /* */
/*                                                                  */
/********************************************************************/

/********************* do linewise with stat.c **********************/

#  include "amiga/stat.c"
/* this is the exact same stat.c used by Zip */
#endif /* !__SASC */
/* SAS/C makes separate object modules of these; there is less  */
/* trouble that way when redefining standard library functions. */

#include <stdio.h>

void _abort(void)               /* called when ^C is pressed */
{
    /* echon(); */
    close_leftover_open_dirs();
    fflush(stdout);
    fputs("\n^C\n", stderr);
    exit(1);
}


/************************************************************/
/* function screensize() -- uses sendpkt() from filedate.c: */
/************************************************************/

#include <devices/conunit.h>
#include <dos/dosextens.h>
#include <exec/memory.h>
#include <clib/exec_protos.h>

extern long sendpkt(struct MsgPort *pid, long action, long *args, long nargs);

int screensize(int *ttrows, int *ttcols)
{
    BPTR fh = Output();
    if (fh && IsInteractive(fh)) {
        struct ConUnit *conunit = NULL;
        void *conp = ((struct FileHandle *) (fh << 2))->fh_Type;
        struct InfoData *ind = AllocMem(sizeof(*ind), MEMF_PUBLIC);
        long argp = ((unsigned long) ind) >> 2;

        if (ind && conp && sendpkt(conp, ACTION_DISK_INFO, &argp, 1))
            conunit = (void *) ((struct IOStdReq *) ind->id_InUse)->io_Unit;
        if (ind)
            FreeMem(ind, sizeof(*ind));
        if (conunit) {
            if (ttrows) *ttrows = conunit->cu_YMax + 1;
            if (ttcols) *ttcols = conunit->cu_XMax + 1;
            return 0;     /* success */
        }
    }
    if (ttrows) *ttrows = INT_MAX;
    if (ttcols) *ttcols = INT_MAX;
    return 1;             /* failure */
}


#ifdef AMIGA_VOLUME_LABELS
/* This function is for if we someday implement -$ on the Amiga. */
#  include <dos/dosextens.h>
#  include <dos/filehandler.h>
#  include <clib/macros.h>

BOOL is_floppy(ZCONST char *path)
{
    BOOL okay = FALSE;
    char devname[32], *debna;
    ushort i;
    BPTR lok = Lock((char *)path, ACCESS_READ), pok;
    struct FileSysStartupMsg *fart;
    struct DeviceNode *debb, devlist = (void *) BADDR((struct DosInfo *)
                                BADDR(DOSBase->dl_Root->rn_Info)->di_DevInfo);
    if (!lok)
        return FALSE;                   /* should not happen */
    if (pok = ParentDir((char *)path)) {
        UnLock(lok);
        UnLock(pok);
        return FALSE;                   /* it's not a root directory path */
    }
    Forbid();
    for (debb = devlist; debb; debb = BADDR(debb->dn_Next))
        if (debb->dn_Type == DLT_DEVICE && (debb->dn_Task == lick->fl_Task))
            if (fart = BADDR(debb->dn_Startup)) {
                debna = (char *) BADDR(fart->fssm_Device) + 1;
                if ((i = debna[-1]) > 31) i = 30;
                strncpy(devname, debna, i);
                devname[i] = 0;
                okay = !strcmp(devname, "trackdisk.device")
                                || !strcmp(devname, "mfm.device")
                                || !strcmp(devname, "messydisk.device");
                break;  /* We only support obvious floppy drives, not tricky */
            }           /* things like removable cartrige hard drives, or    */
    Permit();           /* any unusual kind of floppy device driver.         */
    return okay;
}
#endif /* AMIGA_VOLUME_LABELS */


#ifndef SFX

# if 0
/* As far as I can tell, all the locales AmigaDOS 2.1 knows about all */
/* happen to use DF_MDY ordering, so there's no point in using this.  */

/*************************/
/* Function dateformat() */
/*************************/

#include <clib/locale_protos.h>
#ifdef AZTEC_C
#  include <pragmas/locale_lib.h>
#endif

int dateformat()
{
/*---------------------------------------------------------------------------
    For those operating systems which support it, this function returns a
    value which tells how national convention says that numeric dates are
    displayed.  Return values are DF_YMD, DF_DMY and DF_MDY (the meanings
    should be fairly obvious).
  ---------------------------------------------------------------------------*/
    struct Library *LocaleBase;
    struct Locale *ll;
    int result = DF_MDY;        /* the default */

    if ((LocaleBase = OpenLibrary("locale.library", 0))) {
        if (ll = OpenLocale(NULL)) {
            uch *f = ll->loc_ShortDateFormat;
            /* In this string, %y|%Y is year, %b|%B|%h|%m is month, */
            /* %d|%e is day day, and %D|%x is short for mo/da/yr.   */
            if (!strstr(f, "%D") && !strstr(f, "%x")) {
                uch *da, *mo, *yr;
                if (!(mo = strstr(f, "%b")) && !(mo = strstr(f, "%B"))
                                    && !(mo = strstr(f, "%h")))
                    mo = strstr(f, "%m");
                if (!(da = strstr(f, "%d")))
                    da = strstr(f, "%e");
                if (!(yr = strstr(f, "%y")))
                    yr = strstr(f, "%Y");
                if (yr && yr < mo)
                    result = DF_YMD;
                else if (da && da < mo)
                    result = DF_DMY;
            }
            CloseLocale(ll);
        }
        CloseLibrary(LocaleBase);
    }
    return result;
}

# endif /* 0 */


/************************/
/*  Function version()  */
/************************/


/* NOTE:  the following include depends upon the environment
 *        variable $Workbench to be set correctly.  (Set by
 *        default, by kickstart during startup)
 */
int WBversion = (int)
#include "ENV:Workbench"
;

void version(__G)
   __GDEF
{
/* Define buffers. */

   char buf1[16];  /* compiler name */
   char buf2[16];  /* revstamp */
   char buf3[16];  /* OS */
   char buf4[16];  /* Date */
/*   char buf5[16];  /* Time */

/* format "with" name strings */

#ifdef AMIGA
# ifdef __SASC
   strcpy(buf1,"SAS/C ");
# else
#  ifdef LATTICE
    strcpy(buf1,"Lattice C ");
#  else
#   ifdef AZTEC_C
     strcpy(buf1,"Manx Aztec C ");
#   else
     strcpy(buf1,"UNKNOWN ");
#   endif
#  endif
# endif
/* "under" */
  sprintf(buf3,"AmigaDOS v%d",WBversion);
#else
  strcpy(buf1,"Unknown compiler ");
  strcpy(buf3,"Unknown OS");
#endif

/* Define revision, date, and time strings.
 * NOTE:  Do not calculate run time, be sure to use time compiled.
 * Pass these strings via your makefile if undefined.
 */

#if defined(__VERSION__) && defined(__REVISION__)
  sprintf(buf2,"version %d.%d",__VERSION__,__REVISION__);
#else
# ifdef __VERSION__
  sprintf(buf2,"version %d",__VERSION__);
# else
  sprintf(buf2,"unknown version");
# endif
#endif

#ifdef __DATE__
  sprintf(buf4," on %s",__DATE__);
#else
  strcpy(buf4," unknown date");
#endif

/******
#ifdef __TIME__
  sprintf(buf5," at %s",__TIME__);
#else
  strcpy(buf5," unknown time");
#endif
******/

/* Print strings using "CompiledWith" mask defined in unzip.c (used by all).
 *  ("Compiled with %s%s for %s%s%s%s.")
 */

   printf(LoadFarString(CompiledWith),
     buf1,
     buf2,
     buf3,
     buf4,
     "",    /* buf5 not used */
     "" );  /* buf6 not used */

} /* end function version() */

#endif /* !SFX */
