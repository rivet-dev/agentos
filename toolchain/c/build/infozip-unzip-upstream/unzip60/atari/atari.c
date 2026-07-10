/*
  Copyright (c) 1990-2008 Info-ZIP.  All rights reserved.

  See the accompanying file LICENSE, version 2000-Apr-09 or later
  (the contents of which are also included in unzip.h) for terms of use.
  If, for some reason, all these files are missing, the Info-ZIP license
  also may be found at:  ftp://ftp.info-zip.org/pub/infozip/license.html
*/
/*---------------------------------------------------------------------------

  atari.c

  Atari-specific routines for use with Info-ZIP's UnZip 5.1 and later.

  Contains:  readdir()
             do_wild()           <-- generic enough to put in fileio.c?
             mapattr()
             mapname()
             checkdir()
             mkdir()
             close_outfile()
             stamp_file()        [optional feature]
             version()

  Due to the amazing MiNT library being very, very close to BSD unix's
  library, I'm using the unix.c as a base for this.  Note:  If you're not
  going to compile this with the MiNT libraries (for GNU C, Turbo C, Pure C,
  Lattice C, or Heat & Serve C), you're going to be in for some nasty work.
  Most of the modifications in this file were made by Chris Herborth
  (cherborth@semprini.waterloo-rdp.on.ca) and /should/ be marked with [cjh].

  ---------------------------------------------------------------------------*/


#define UNZIP_INTERNAL
#include "unzip.h"
#include <dirent.h>            /* MiNTlibs has dirent [cjh] */

static int created_dir;        /* used in mapname(), checkdir() */
static int renamed_fullpath;   /* ditto */


#ifndef SFX

/**********************/
/* Function do_wild() */   /* for porting:  dir separator; match(ignore_case) */
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
            dir = NULL;
            return matchname;
        }

        /* break the wildspec into a directory part and a wildcard filename */
        if ((wildname = strrchr(wildspec, '/')) == (ZCONST char *)NULL) {
            dirname = ".";
            dirnamelen = 1;
            have_dirname = FALSE;
            wildname = wildspec;
        } else {
            ++wildname;     /* point at character after '/' */
            dirnamelen = wildname - wildspec;
            if ((dirname = (char *)malloc(dirnamelen+1)) == (char *)NULL) {
                Info(slide, 0x201, ((char *)slide,
                  "warning:  cannot allocate wildcard buffers\n"));
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
                Trace((stderr, "do_wild:  readdir returns %s\n", file->d_name));
                if (file->d_name[0] == '.' && wildname[0] != '.')
                    continue; /* Unix:  '*' and '?' do not match leading dot */
                    /* Need something here for TOS filesystem? [cjh] */
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
        /* May need special TOS handling here. [cjh] */
        Trace((stderr, "do_wild:  readdir returns %s\n", file->d_name));
        if (file->d_name[0] == '.' && wildname[0] != '.')
            continue;   /* Unix:  '*' and '?' do not match leading dot */
        if (match(file->d_name, wildname, 0 WISEP)) {   /* 0 = case sens. */
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
    int r;
    ulg tmp = G.crec.external_file_attributes;

    G.pInfo->file_attr = 0;
    /* initialized to 0 for check in "default" branch below... */

    switch (G.pInfo->hostnum) {
        case AMIGA_:
            tmp = (unsigned)(tmp>>17 & 7);   /* Amiga RWE bits */
            G.pInfo->file_attr = (unsigned)(tmp<<6 | tmp<<3 | tmp);
            break;
        case THEOS_:
            tmp &= 0xF1FFFFFFL;
            if ((tmp & 0xF0000000L) != 0x40000000L)
                tmp &= 0x01FFFFFFL;     /* not a dir, mask all ftype bits */
            else
                tmp &= 0x41FFFFFFL;     /* leave directory bit as set */
            /* fall through! */
        case UNIX_:
        case VMS_:
        case ACORN_:
        case ATARI_:
        case ATHEOS_:
        case BEOS_:
        case QDOS_:
        case TANDEM_:
            r = FALSE;
            G.pInfo->file_attr = (unsigned)(tmp >> 16);
            if (G.pInfo->file_attr == 0 && G.extra_field) {
                /* Some (non-Info-ZIP) implementations of Zip for Unix and
                 * VMS (and probably others ??) leave 0 in the upper 16-bit
                 * part of the external_file_attributes field. Instead, they
                 * store file permission attributes in some extra field.
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
                            G.pInfo->file_attr =
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
                G.pInfo->symlink = S_ISLNK(G.pInfo->file_attr) &&
                                   SYMLINK_HOST(G.pInfo->hostnum);
#endif
                return 0;
            }
            /* fall through! */
        /* all remaining cases:  expand MSDOS read-only bit into write perms */
        case FS_FAT_:
            /* PKWARE's PKZip for Unix marks entries as FS_FAT_, but stores the
             * Unix attributes in the upper 16 bits of the external attributes
             * field, just like Info-ZIP's Zip for Unix.  We try to use that
             * value, after a check for consistency with the MSDOS attribute
             * bits (see below).
             */
            G.pInfo->file_attr = (unsigned)(tmp >> 16);
            /* fall through! */
        case FS_HPFS_:
        case FS_NTFS_:
        case MAC_:
        case TOPS20_:
        default:
            /* Ensure that DOS subdir bit is set when the entry's name ends
             * in a '/'.  Some third-party Zip programs fail to set the subdir
             * bit for directory entries.
             */
            if ((tmp & 0x10) == 0) {
                extent fnlen = strlen(G.filename);
                if (fnlen > 0 && G.filename[fnlen-1] == '/')
                    tmp |= 0x10;
            }
            /* read-only bit --> write perms; subdir bit --> dir exec bit */
            tmp = !(tmp & 1) << 1  |  (tmp & 0x10) >> 4;
            if ((G.pInfo->file_attr & 0700) == (unsigned)(0400 | tmp<<6)) {
                /* keep previous G.pInfo->file_attr setting, when its "owner"
                 * part appears to be consistent with DOS attribute flags!
                 */
#ifdef SYMLINKS
                /* Entries "made by FS_FAT_" could have been zipped on a
                 * system that supports POSIX-style symbolic links.
                 */
                G.pInfo->symlink = S_ISLNK(G.pInfo->file_attr) &&
                                   (G.pInfo->hostnum == FS_FAT_);
#endif
                return 0;
            }
            G.pInfo->file_attr = (unsigned)(0444 | tmp<<6 | tmp<<3 | tmp);
            break;
    } /* end switch (host-OS-created-by) */

    /* for originating systems with no concept of "group," "other," "system": */
    umask( (int)(tmp=umask(0)) );    /* apply mask to expanded r/w(/x) perms */
    G.pInfo->file_attr &= ~tmp;

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
#ifdef ACORN_FTYPE_NFS
    char *lastcomma=(char *)NULL;  /* pointer to last comma in pathcomp */
    RO_extra_block *ef_spark;      /* pointer Acorn FTYPE ef block */
#endif
    int killed_ddot = FALSE;       /* is set when skipping "../" pathcomp */
    int error = MPN_OK;
    register unsigned workch;      /* hold the character being tested */


/*---------------------------------------------------------------------------
    Initialize various pointers and counters and stuff.
  ---------------------------------------------------------------------------*/

    if (G.pInfo->vollabel)
        return MPN_VOL_LABEL;   /* can't set disk volume labels on Atari */

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
                lastsemi = pp;
                *pp++ = ';';      /* keep for now; remove VMS ";##" */
                break;            /*  later, if requested */

#ifdef ACORN_FTYPE_NFS
            case ',':             /* NFS filetype extension */
                lastcomma = pp;
                *pp++ = ',';      /* keep for now; may need to remove */
                break;            /*  later, if requested */
#endif

#ifdef MTS
            case ' ':             /* change spaces to underscore under */
                *pp++ = '_';      /*  MTS; leave as spaces under Unix */
                break;
#endif

            default:
                /* allow European characters in filenames: */
                if (isprint(workch) || (128 <= workch && workch <= 254))
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

    /* On UNIX (and compatible systems), "." and ".." are reserved for
     * directory navigation and cannot be used as regular file names.
     * These reserved one-dot and two-dot names are mapped to "_" and "__".
     */
    if (strcmp(pathcomp, ".") == 0)
        *pathcomp = '_';
    else if (strcmp(pathcomp, "..") == 0)
        strcpy(pathcomp, "__");

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

    if (*pathcomp == '\0') {
        Info(slide, 1, ((char *)slide, "mapname:  conversion of %s failed\n",
          FnFilter1(G.filename)));
        return (error & ~MPN_MASK) | MPN_ERR_SKIP;
    }

    checkdir(__G__ pathcomp, APPEND_NAME);  /* returns 1 if truncated: care? */
    checkdir(__G__ G.filename, GETPATH);

    return error;

} /* end function mapname() */




#if 0  /*========== NOTES ==========*/

  extract-to dir:      a:path/
  buildpath:           path1/path2/ ...   (NULL-terminated)
  pathcomp:                filename

  mapname():
    loop over chars in zipfile member name
      checkdir(path component, COMPONENT | CREATEDIR) --> map as required?
        (d:/tmp/unzip/)                    (disk:[tmp.unzip.)
        (d:/tmp/unzip/jj/)                 (disk:[tmp.unzip.jj.)
        (d:/tmp/unzip/jj/temp/)            (disk:[tmp.unzip.jj.temp.)
    finally add filename itself and check for existence? (could use with rename)
        (d:/tmp/unzip/jj/temp/msg.outdir)  (disk:[tmp.unzip.jj.temp]msg.outdir)
    checkdir(name, GETPATH)     -->  copy path to name and free space

#endif /* 0 */




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
/* SHORT_NAMES required for TOS, but it has to co-exist for minix fs... [cjh] */
#ifdef SHORT_NAMES
        char *old_end = end;
#endif

        Trace((stderr, "appending dir segment [%s]\n", FnFilter1(pathcomp)));
        while ((*end = *pathcomp++) != '\0')
            ++end;
/* SHORT_NAMES required for TOS, but it has to co-exist for minix fs... [cjh] */
#ifdef SHORT_NAMES   /* path components restricted to 14 chars, typically */
        if ((end-old_end) > FILENAME_MAX)  /* GRR:  proper constant? */
            *(end = old_end + FILENAME_MAX) = '\0';
#endif

        /* GRR:  could do better check, see if overrunning buffer as we go:
         * check end-buildpath after each append, set warning variable if
         * within 20 of FILNAMSIZ; then if var set, do careful check when
         * appending.  Clear variable when begin new path. */

        if ((end-buildpath) > FILNAMSIZ-3)  /* need '/', one-char name, '\0' */
            too_long = TRUE;                /* check if extracting directory? */
        if (stat(buildpath, &G.statbuf)) {  /* path doesn't exist */
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
              FnFilter2(buildpath), FnFilter(G.filename)));
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
/* SHORT_NAMES required for TOS, but it has to co-exist for minix fs... [cjh] */
#ifdef SHORT_NAMES
        char *old_end = end;
#endif

        Trace((stderr, "appending filename [%s]\n", FnFilter1(pathcomp)));
        while ((*end = *pathcomp++) != '\0') {
            ++end;
/* SHORT_NAMES required for TOS, but it has to co-exist for minix fs... [cjh] */
#ifdef SHORT_NAMES  /* truncate name at 14 characters, typically */
            if ((end-old_end) > FILENAME_MAX)      /* GRR:  proper constant? */
                *(end = old_end + FILENAME_MAX) = '\0';
#endif
            if ((end-buildpath) >= FILNAMSIZ) {
                *--end = '\0';
                Info(slide, 0x201, ((char *)slide,
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

/* GRR:  for VMS and TOPS-20, add up to 13 to strlen */

    if (FUNCTION == INIT) {
        Trace((stderr, "initializing buildpath to "));
#ifdef ACORN_FTYPE_NFS
        if ((buildpath = (char *)malloc(strlen(G.filename)+rootlen+
                                        (uO.acorn_nfs_ext ? 5 : 1)))
#else
        if ((buildpath = (char *)malloc(strlen(G.filename)+rootlen+1))
#endif
            == (char *)NULL)
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
    ROOT:  if appropriate, store the path in rootpath and create it if
    necessary; else assume it's a zipfile member and return.  This path
    segment gets used in extracting all members from every zipfile specified
    on the command line.
  ---------------------------------------------------------------------------*/

#if (!defined(SFX) || defined(SFX_EXDIR))
    if (FUNCTION == ROOT) {
        Trace((stderr, "initializing root path to [%s]\n", pathcomp));
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
            if (tmproot[rootlen-1] == '/') {
                tmproot[--rootlen] = '\0';
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

void close_outfile(__G)    /* GRR: change to return PK-style warning level */
    __GDEF
{
#ifdef USE_EF_UT_TIME
    unsigned eb_izux_flg;
    iztimes zt;
#endif
    ztimbuf tp;

/*---------------------------------------------------------------------------
    If symbolic links are supported, allocate storage for a symlink control
    structure, put the uncompressed "data" and other required info in it, and
    add the structure to the "deferred symlinks" chain.  Since we know it's a
    symbolic link to start with, we shouldn't have to worry about overflowing
    unsigned ints with unsigned longs.
  ---------------------------------------------------------------------------*/

    /* symlinks allowed on minix filesystems [cjh]
     * Hopefully this will work properly... We won't bother to try if
     * MiNT isn't present; the symlink should fail if we're on a TOS
     * filesystem.
     * BUG: should we copy the original file to the "symlink" if the
     *      link fails?
     */
    if (G.symlnk) {
        extent ucsize = (extent)G.lrec.ucsize;
        /* size of the symlink entry is the sum of
         *  (struct size (includes 1st '\0') + 1 additional trailing '\0'),
         *  system specific attribute data size (might be 0),
         *  and the lengths of name and link target.
         */
        extent slnk_entrysize = (sizeof(slinkentry) + 1) +
                                ucsize + strlen(G.filename);
        slinkentry *slnk_entry;

        if (slnk_entrysize < ucsize) {
            Info(slide, 0x201, ((char *)slide,
              "warning:  symbolic link (%s) failed: mem alloc overflow\n",
              FnFilter1(G.filename)));
            fclose(G.outfile);
            return;
        }

        if ((slnk_entry = (slinkentry *)malloc(slnk_entrysize)) == NULL) {
            Info(slide, 0x201, ((char *)slide,
              "warning:  symbolic link (%s) failed: no mem\n",
              FnFilter1(G.filename)));
            fclose(G.outfile);
            return;
        }
        slnk_entry->next = NULL;
        slnk_entry->targetlen = ucsize;
        slnk_entry->attriblen = 0;      /* don't set attributes for symlinks */
        slnk_entry->target = slnk_entry->buf;
        slnk_entry->fname = slnk_entry->target + ucsize + 1;
        strcpy(slnk_entry->fname, G.filename);

        /* move back to the start of the file to re-read the "link data" */
        rewind(G.outfile);

        if (fread(slnk_entry->target, 1, ucsize, G.outfile) != ucsize)
        {
            Info(slide, 0x201, ((char *)slide,
              "warning:  symbolic link (%s) failed\n",
              FnFilter1(G.filename)));
            free(slnk_entry);
            fclose(G.outfile);
            return;
        }
        fclose(G.outfile);                  /* close "link" file for good... */
        slnk_entry->target[ucsize] = '\0';
        if (QCOND2)
            Info(slide, 0, ((char *)slide, "-> %s ",
              FnFilter1(slnk_entry->target)));
        /* add this symlink record to the list of deferred symlinks */
        if (G.slink_last != NULL)
            G.slink_last->next = slnk_entry;
        else
            G.slink_head = slnk_entry;
        G.slink_last = slnk_entry;
        return;
    }

    fclose(G.outfile);

/*---------------------------------------------------------------------------
    Convert from MSDOS-format local time and date to Unix-format 32-bit GMT
    time:  adjust base year from 1980 to 1970, do usual conversions from
    yy/mm/dd hh:mm:ss to elapsed seconds, and account for timezone and day-
    light savings time differences.
  ---------------------------------------------------------------------------*/

    /* skip restoring time stamps on user's request */
    if (uO.D_flag <= 1) {
#ifdef USE_EF_UT_TIME
        eb_izux_flg = (G.extra_field
#ifdef IZ_CHECK_TZ
                       && G.tz_is_valid
#endif
                       ? ef_scan_for_izux(G.extra_field,
                           G.lrec.extra_field_length, 0,
                           G.lrec.last_mod_dos_datetime, &zt, NULL)
                       : 0);
        if (eb_izux_flg & EB_UT_FL_MTIME) {
            tp.modtime = zt.mtime;
            TTrace((stderr,
              "\nclose_outfile:  Unix e.f. modif. time = %ld\n",
              tp.modtime));
        } else {
            tp.modtime = dos_to_unix_time(G.lrec.last_mod_dos_datetime);
        }
        if (eb_izux_flg & EB_UT_FL_ATIME) {
            tp.actime = zt.atime;
            TTrace((stderr,
              "close_outfile:  Unix e.f. access time = %ld\n",
              tp.actime));
        } else {
            tp.actime = tp.modtime;
            TTrace((stderr,
              "\nclose_outfile:  modification/access times = %ld\n",
              tp.modtime));
        }
#else /* !USE_EF_UT_TIME */
        tp.actime = tp.modtime
          = dos_to_unix_time(G.lrec.last_mod_dos_datetime);

        TTrace((stderr, "\nclose_outfile:  modification/access times = %ld\n",
          tp.modtime));
#endif /* ?USE_EF_UT_TIME */

        /* set the file's access and modification times */
        if (utime(G.filename, &tp))
            Info(slide, 0x201, ((char *)slide,
              "warning:  cannot set the time for %s\n",
              FnFilter1(G.filename)));
    }

/*---------------------------------------------------------------------------
    Change the file permissions from default ones to those stored in the
    zipfile.
  ---------------------------------------------------------------------------*/

#ifndef NO_CHMOD
    if (chmod(G.filename, 0xffff & G.pInfo->file_attr))
        perror("chmod (file attributes) error");
#endif

} /* end function close_outfile() */




#ifdef TIMESTAMP

/***************************/
/*  Function stamp_file()  */
/***************************/

int stamp_file(fname, modtime)
    ZCONST char *fname;
    time_t modtime;
{
    ztimbuf tp;

    tp.modtime = tp.actime = modtime;
    return (utime(fname, &tp));

} /* end function stamp_file() */

#endif /* TIMESTAMP */




#ifndef SFX

/************************/
/*  Function version()  */
/************************/

void version(__G)
    __GDEF
{
#ifdef __TURBOC__
    char buf[40];
#endif

    sprintf((char *)slide, LoadFarString(CompiledWith),

#ifdef __GNUC__
      "gcc ", __VERSION__,
#else
#  if 0
      "cc ", (sprintf(buf, " version %d", _RELEASE), buf),
#  else
#  ifdef __TURBOC__
      "Turbo C", (sprintf(buf, " (0x%04x = %d)", __TURBOC__, __TURBOC__), buf),
#  else
      "unknown compiler", "",
#  endif
#  endif
#endif

#ifdef __MINT__
      "Atari TOS/MiNT",
#else
      "Atari TOS",
#endif

      " (Atari ST/TT/Falcon030)",

#ifdef __DATE__
      " on ", __DATE__
#else
      "", ""
#endif
    );

    (*G.message)((zvoid *)&G, slide, (ulg)strlen((char *)slide), 0);

} /* end function version() */

#endif /* !SFX */
