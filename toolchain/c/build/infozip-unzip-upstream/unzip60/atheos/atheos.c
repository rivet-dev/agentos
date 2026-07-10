/*
  Copyright (c) 1990-2008 Info-ZIP.  All rights reserved.

  See the accompanying file LICENSE, version 2000-Apr-09 or later
  (the contents of which are also included in unzip.h) for terms of use.
  If, for some reason, all these files are missing, the Info-ZIP license
  also may be found at:  ftp://ftp.info-zip.org/pub/infozip/license.html
*/
/*---------------------------------------------------------------------------

  atheos.c by Ruslan Nickolaev (nruslan@hotbox.ru)

  AtheOS-specific routines for use with Info-ZIP's UnZip 5.51 and later.
  (based on beos/beos.c and unix/unix.c)

  Contains:  do_wild()           <-- generic enough to put in fileio.c?
             mapattr()
             mapname()
             checkdir()
             close_outfile()
             defer_dir_attribs()
             set_direc_attribs()
             stamp_file()
             version()
             scanAtheOSexfield()
             set_file_attrs()
             setAtheOSexfield()

  ---------------------------------------------------------------------------*/


#define UNZIP_INTERNAL
#include "unzip.h"

#include "atheos.h"
#include <errno.h>             /* Just make sure we've got a few things... */
#include <sys/types.h>
#include <fcntl.h>
#include <dirent.h>
#include <atheos/fs_attribs.h>

static unsigned filtattr OF((__GPRO__ unsigned perms));
static uch *scanAtheOSexfield  OF((const uch *ef_ptr, unsigned ef_len));
static int  set_file_attrs( const char *, const unsigned char *, const off_t );
static void setAtheOSexfield   OF((const char *path, uch *extra_field));

#ifdef SET_DIR_ATTRIB
typedef struct uxdirattr {      /* struct for holding unix style directory */
    struct uxdirattr *next;     /*  info until can be sorted and set at end */
    char *fn;                   /* filename of directory */
    union {
        iztimes t3;             /* mtime, atime, ctime */
        ztimbuf t2;             /* modtime, actime */
    } u;
    unsigned perms;             /* same as min_info.file_attr */
    int have_uidgid;            /* flag */
    ulg uidgid[2];
    char fnbuf[1];              /* buffer stub for directory name */
} uxdirattr;
#define UxAtt(d)  ((uxdirattr *)d)    /* typecast shortcut */
#endif /* SET_DIR_ATTRIB */

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

/* static int created_dir;      */      /* used in mapname(), checkdir() */
/* static int renamed_fullpath; */      /* ditto */

#ifndef SFX

/**********************/
/* Function do_wild() */   /* for porting: dir separator; match(ignore_case) */
/**********************/

char *do_wild(__G__ wildspec)
    __GDEF
    ZCONST char *wildspec;  /* only used first time on a given dir */
{
/* these statics are now declared in SYSTEM_SPECIFIC_GLOBALS in athcfg.h:
    static DIR *wild_dir = (DIR *)NULL;
    static ZCONST char *wildname;
    static char *dirname, matchname[FILNAMSIZ];
    static int notfirstcall=FALSE, have_dirname, dirnamelen;
*/
    struct dirent *file;

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
        if ((G.wildname = (ZCONST char *)strrchr(wildspec, '/')) == NULL) {
            G.dirname = ".";
            G.dirnamelen = 1;
            G.have_dirname = FALSE;
            G.wildname = wildspec;
        } else {
            ++G.wildname;     /* point at character after '/' */
            G.dirnamelen = G.wildname - wildspec;
            if ((G.dirname = (char *)malloc(G.dirnamelen+1)) == (char *)NULL) {
                Info(slide, 0x201, ((char *)slide,
                  "warning:  cannot allocate wildcard buffers\n"));
                strncpy(G.matchname, wildspec, FILNAMSIZ);
                G.matchname[FILNAMSIZ-1] = '\0';
                return G.matchname; /* but maybe filespec was not a wildcard */
            }
            strncpy(G.dirname, wildspec, G.dirnamelen);
            G.dirname[G.dirnamelen] = '\0';   /* terminate for strcpy below */
            G.have_dirname = TRUE;
        }

        if ((G.wild_dir = (zvoid *)opendir(G.dirname)) != (zvoid *)NULL) {
            while ((file = readdir((DIR *)G.wild_dir)) !=
                   (struct dirent *)NULL) {
                Trace((stderr, "do_wild:  readdir returns %s\n",
                  FnFilter1(file->d_name)));
                if (file->d_name[0] == '.' && G.wildname[0] != '.')
                    continue; /* Unix:  '*' and '?' do not match leading dot */
                if (match(file->d_name, G.wildname, 0 WISEP) &&/*0=case sens.*/
                    /* skip "." and ".." directory entries */
                    strcmp(file->d_name, ".") && strcmp(file->d_name, "..")) {
                    Trace((stderr, "do_wild:  match() succeeds\n"));
                    if (G.have_dirname) {
                        strcpy(G.matchname, G.dirname);
                        strcpy(G.matchname+G.dirnamelen, file->d_name);
                    } else
                        strcpy(G.matchname, file->d_name);
                    return G.matchname;
                }
            }
            /* if we get to here directory is exhausted, so close it */
            closedir((DIR *)G.wild_dir);
            G.wild_dir = (zvoid *)NULL;
        }
        Trace((stderr, "do_wild:  opendir(%s) returns NULL\n",
          FnFilter1(G.dirname)));

        /* return the raw wildspec in case that works (e.g., directory not
         * searchable, but filespec was not wild and file is readable) */
        strncpy(G.matchname, wildspec, FILNAMSIZ);
        G.matchname[FILNAMSIZ-1] = '\0';
        return G.matchname;
    }

    /* last time through, might have failed opendir but returned raw wildspec */
    if ((DIR *)G.wild_dir == (DIR *)NULL) {
        G.notfirstcall = FALSE; /* nothing left--reset for new wildspec */
        if (G.have_dirname)
            free(G.dirname);
        return (char *)NULL;
    }

    /* If we've gotten this far, we've read and matched at least one entry
     * successfully (in a previous call), so dirname has been copied into
     * matchname already.
     */
    while ((file = readdir((DIR *)G.wild_dir)) != (struct dirent *)NULL) {
        Trace((stderr, "do_wild:  readdir returns %s\n",
          FnFilter1(file->d_name)));
        if (file->d_name[0] == '.' && G.wildname[0] != '.')
            continue;   /* Unix:  '*' and '?' do not match leading dot */
        if (match(file->d_name, G.wildname, 0 WISEP)) { /* 0 == case sens. */
            Trace((stderr, "do_wild:  match() succeeds\n"));
            if (G.have_dirname) {
                /* strcpy(G.matchname, G.dirname); */
                strcpy(G.matchname+G.dirnamelen, file->d_name);
            } else
                strcpy(G.matchname, file->d_name);
            return G.matchname;
        }
    }

    closedir((DIR *)G.wild_dir);  /* at least one entry read; nothing left */
    G.wild_dir = (zvoid *)NULL;
    G.notfirstcall = FALSE;       /* reset for new wildspec */
    if (G.have_dirname)
        free(G.dirname);
    return (char *)NULL;

} /* end function do_wild() */

#endif /* !SFX */




#ifndef S_ISUID
# define S_ISUID        0004000 /* set user id on execution */
#endif
#ifndef S_ISGID
# define S_ISGID        0002000 /* set group id on execution */
#endif
#ifndef S_ISVTX
# define S_ISVTX        0001000 /* save swapped text even after use */
#endif

/************************/
/*  Function filtattr() */
/************************/
/* This is used to clear or keep the SUID and GID bits on file permissions.
 * It's possible that a file in an archive could have one of these bits set
 * and, unknown to the person unzipping, could allow others to execute the
 * file as the user or group.  The new option -K bypasses this check.
 */

static unsigned filtattr(__G__ perms)
    __GDEF
    unsigned perms;
{
    /* keep setuid/setgid/tacky perms? */
    if (!uO.K_flag)
        perms &= ~(S_ISUID | S_ISGID | S_ISVTX);

    return (0xffff & perms);
} /* end function filtattr() */





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
        case ATHEOS_:
        case UNIX_:
        case VMS_:
        case ACORN_:
        case ATARI_:
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
        return MPN_VOL_LABEL;   /* can't set disk volume labels in AtheOS */

    /* can create path as long as not just freshening, or if user told us */
    G.create_dirs = (!uO.fflag || renamed);

    G.created_dir = FALSE;      /* not yet */

    /* user gave full pathname:  don't prepend rootpath */
    G.renamed_fullpath = (renamed && (*G.filename == '/'));

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
        if (G.created_dir) {
            if (QCOND2) {
                Info(slide, 0, ((char *)slide, "   creating: %s\n",
                  FnFilter1(G.filename)));
            }

            if (!uO.J_flag) {   /* Handle the AtheOS extra field if present. */
                void *ptr = scanAtheOSexfield(G.extra_field,
                                              G.lrec.extra_field_length);
                if (ptr) {
                    setAtheOSexfield( G.filename, ptr );
                }
            }

#ifndef NO_CHMOD
            /* set approx. dir perms (make sure can still read/write in dir) */
            if (chmod(G.filename, filtattr(__G__ G.pInfo->file_attr) | 0700))
                perror("chmod (directory attributes) error");
#endif

            /* set dir time (note trailing '/') */
            return (error & ~MPN_MASK) | MPN_CREATED_DIR;
        }
        if (!uO.J_flag) {   /* Handle the AtheOS extra field if present. */
            void *ptr = scanAtheOSexfield(G.extra_field,
                                          G.lrec.extra_field_length);
            if (ptr) {
                setAtheOSexfield(G.filename, ptr);
            }
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
 /* static int rootlen = 0; */  /* length of rootpath */
 /* static char *rootpath;  */  /* user's "extract-to" directory */
 /* static char *buildpath; */  /* full path (so far) to extracted file */
 /* static char *end;       */  /* pointer to end of buildpath ('\0') */

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
        while ((*G.end = *pathcomp++) != '\0')
            ++G.end;
#ifdef SHORT_NAMES   /* path components restricted to 14 chars, typically */
        if ((G.end-old_end) > FILENAME_MAX)  /* GRR:  proper constant? */
            *(G.end = old_end + FILENAME_MAX) = '\0';
#endif

        /* GRR:  could do better check, see if overrunning buffer as we go:
         * check end-buildpath after each append, set warning variable if
         * within 20 of FILNAMSIZ; then if var set, do careful check when
         * appending.  Clear variable when begin new path. */

        /* next check: need to append '/', at least one-char name, '\0' */
        if ((G.end-G.buildpath) > FILNAMSIZ-3)
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
            if (mkdir(G.buildpath, 0777) == -1) {   /* create the directory */
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
        *G.end++ = '/';
        *G.end = '\0';
        Trace((stderr, "buildpath now = [%s]\n", FnFilter1(G.buildpath)));
        return MPN_OK;

    } /* end if (FUNCTION == APPEND_DIR) */

/*---------------------------------------------------------------------------
    GETPATH:  copy full path to the string pointed at by pathcomp, and free
    G.buildpath.
  ---------------------------------------------------------------------------*/

    if (FUNCTION == GETPATH) {
        strcpy(pathcomp, G.buildpath);
        Trace((stderr, "getting and freeing path [%s]\n",
          FnFilter1(pathcomp)));
        free(G.buildpath);
        G.buildpath = G.end = (char *)NULL;
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
        while ((*G.end = *pathcomp++) != '\0') {
            ++G.end;
#ifdef SHORT_NAMES  /* truncate name at 14 characters, typically */
            if ((G.end-old_end) > FILENAME_MAX)    /* GRR:  proper constant? */
                *(G.end = old_end + FILENAME_MAX) = '\0';
#endif
            if ((G.end-G.buildpath) >= FILNAMSIZ) {
                *--G.end = '\0';
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

/* GRR:  for VMS and TOPS-20, add up to 13 to strlen */

    if (FUNCTION == INIT) {
        Trace((stderr, "initializing buildpath to "));
#ifdef ACORN_FTYPE_NFS
        if ((G.buildpath = (char *)malloc(strlen(G.filename)+G.rootlen+
                                          (uO.acorn_nfs_ext ? 5 : 1)))
#else
        if ((G.buildpath = (char *)malloc(strlen(G.filename)+G.rootlen+1))
#endif
            == (char *)NULL)
            return MPN_NOMEM;
        if ((G.rootlen > 0) && !G.renamed_fullpath) {
            strcpy(G.buildpath, G.rootpath);
            G.end = G.buildpath + G.rootlen;
        } else {
            *G.buildpath = '\0';
            G.end = G.buildpath;
        }
        Trace((stderr, "[%s]\n", FnFilter1(G.buildpath)));
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
          FnFilter1(pathcomp)));
        if (pathcomp == (char *)NULL) {
            G.rootlen = 0;
            return MPN_OK;
        }
        if (G.rootlen > 0)      /* rootpath was already set, nothing to do */
            return MPN_OK;
        if ((G.rootlen = strlen(pathcomp)) > 0) {
            char *tmproot;

            if ((tmproot = (char *)malloc(G.rootlen+2)) == (char *)NULL) {
                G.rootlen = 0;
                return MPN_NOMEM;
            }
            strcpy(tmproot, pathcomp);
            if (tmproot[G.rootlen-1] == '/') {
                tmproot[--G.rootlen] = '\0';
            }
            if (G.rootlen > 0 && (SSTAT(tmproot, &G.statbuf) ||
                                  !S_ISDIR(G.statbuf.st_mode)))
            {   /* path does not exist */
                if (!G.create_dirs /* || iswild(tmproot) */ ) {
                    free(tmproot);
                    G.rootlen = 0;
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
                    G.rootlen = 0;
                    /* path didn't exist, tried to create, and failed: */
                    /* file exists, or 2+ subdir levels required */
                    return MPN_ERR_SKIP;
                }
            }
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




static int get_extattribs OF((__GPRO__ iztimes *pzt, ulg z_uidgid[2]));

static int get_extattribs(__G__ pzt, z_uidgid)
    __GDEF
    iztimes *pzt;
    ulg z_uidgid[2];
{
/*---------------------------------------------------------------------------
    Convert from MSDOS-format local time and date to Unix-format 32-bit GMT
    time:  adjust base year from 1980 to 1970, do usual conversions from
    yy/mm/dd hh:mm:ss to elapsed seconds, and account for timezone and day-
    light savings time differences.  If we have a Unix extra field, however,
    we're laughing:  both mtime and atime are ours.  On the other hand, we
    then have to check for restoration of UID/GID.
  ---------------------------------------------------------------------------*/
    int have_uidgid_flg;
    unsigned eb_izux_flg;

    eb_izux_flg = (G.extra_field ? ef_scan_for_izux(G.extra_field,
                   G.lrec.extra_field_length, 0, G.lrec.last_mod_dos_datetime,
#ifdef IZ_CHECK_TZ
                   (G.tz_is_valid ? pzt : NULL),
#else
                   pzt,
#endif
                   z_uidgid) : 0);
    if (eb_izux_flg & EB_UT_FL_MTIME) {
        TTrace((stderr, "\nget_extattribs:  Unix e.f. modif. time = %ld\n",
          pzt->mtime));
    } else {
        pzt->mtime = dos_to_unix_time(G.lrec.last_mod_dos_datetime);
    }
    if (eb_izux_flg & EB_UT_FL_ATIME) {
        TTrace((stderr, "get_extattribs:  Unix e.f. access time = %ld\n",
          pzt->atime));
    } else {
        pzt->atime = pzt->mtime;
        TTrace((stderr, "\nget_extattribs:  modification/access times = %ld\n",
          pzt->mtime));
    }

    /* if -X option was specified and we have UID/GID info, restore it */
    have_uidgid_flg =
#ifdef RESTORE_UIDGID
            (uO.X_flag && (eb_izux_flg & EB_UX2_VALID));
#else
            0;
#endif
    return have_uidgid_flg;
}





/****************************/
/* Function close_outfile() */
/****************************/

void close_outfile(__G)    /* GRR: change to return PK-style warning level */
    __GDEF
{
    union {
        iztimes t3;             /* mtime, atime, ctime */
        struct utimbuf t2;      /* modtime, actime */
    } zt;
    ulg z_uidgid[2];
    int have_uidgid_flg;

/*---------------------------------------------------------------------------
    If symbolic links are supported, allocate storage for a symlink control
    structure, put the uncompressed "data" and other required info in it, and
    add the structure to the "deferred symlinks" chain.  Since we know it's a
    symbolic link to start with, we shouldn't have to worry about overflowing
    unsigned ints with unsigned longs.
  ---------------------------------------------------------------------------*/

#ifdef SYMLINKS
    if (G.symlnk) {
        extent ucsize = (extent)G.lrec.ucsize;
        unsigned AtheOSef_len = 0;
        extent slnk_entrysize;
        uch *AtheOS_exfld = NULL;
        slinkentry *slnk_entry;

        if (!uO.J_flag) {
            /* attributes for symlinks are supported too */
            AtheOS_exfld = scanAtheOSexfield(G.extra_field,
                                             G.lrec.extra_field_length);
            if (AtheOS_exfld) {
                AtheOSef_len = makeword(EB_LEN + AtheOS_exfld) + EB_HEADSIZE;
            }
        }

        /* size of the symlink entry is the sum of
         *  (struct size (includes 1st '\0') + 1 additional trailing '\0'),
         *  system specific attribute data size (might be 0),
         *  and the lengths of name and link target.
         */
        slnk_entrysize = (sizeof(slinkentry) + 1) + AtheOSef_len +
                         ucsize + strlen(G.filename);

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
        slnk_entry->attriblen = AtheOSef_len;
        slnk_entry->target = slnk_entry->buf + AtheOSef_len;
        slnk_entry->fname = slnk_entry->target + ucsize + 1;
        strcpy(slnk_entry->fname, G.filename);
        if (AtheOSef_len > 0)
            /* AtheOS_exfld should not be NULL because AtheOSef_len > 0 */
            memcpy(slnk_entry->buf, AtheOS_exfld, AtheOSef_len);

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
#endif /* SYMLINKS */

    fclose(G.outfile);

    /* handle the AtheOS extra field if present */
    if (!uO.J_flag) {
        void *ptr = scanAtheOSexfield(G.extra_field,
                                      G.lrec.extra_field_length);

        if (ptr) {
            setAtheOSexfield(G.filename, ptr);
        }
    }

    have_uidgid_flg = get_extattribs(__G__ &(zt.t3), z_uidgid);

    /* if -X option was specified and we have UID/GID info, restore it */
    if (have_uidgid_flg) {
        TTrace((stderr, "close_outfile:  restoring Unix UID/GID info\n"));
        if (chown(G.filename, (uid_t)z_uidgid[0], (gid_t)z_uidgid[1]))
        {
            if (uO.qflag)
                Info(slide, 0x201, ((char *)slide,
                  "warning:  cannot set UID %lu and/or GID %lu for %s\n",
                  z_uidgid[0], z_uidgid[1], FnFilter1(G.filename)));
            else
                Info(slide, 0x201, ((char *)slide,
                  " (warning) cannot set UID %lu and/or GID %lu",
                  z_uidgid[0], z_uidgid[1]));
        }
    }

    /* skip restoring time stamps on user's request */
    if (uO.D_flag <= 1) {
        /* set the file's access and modification times */
        if (utime(G.filename, &(zt.t2))) {
            if (uO.qflag)
                Info(slide, 0x201, ((char *)slide,
                  "warning:  cannot set times for %s\n",
                  FnFilter1(G.filename)));
            else
                Info(slide, 0x201, ((char *)slide,
                  " (warning) cannot set times"));
        }
    }

/*---------------------------------------------------------------------------
    Change the file permissions from default ones to those stored in the
    zipfile.
  ---------------------------------------------------------------------------*/

#ifndef NO_CHMOD
    if (chmod(G.filename, filtattr(__G__ G.pInfo->file_attr)))
        perror("chmod (file attributes) error");
#endif

} /* end function close_outfile() */




#ifdef SYMLINKS
int set_symlnk_attribs(__G__ slnk_entry)
    __GDEF
    slinkentry *slnk_entry;
{
    if (slnk_entry->attriblen > 0)
        setAtheOSexfield(slnk_entry->fname, (uch *)slnk_entry->buf);
    /* currently, no error propagation... */
    return PK_OK;
}
#endif /* SYMLINKS */




#ifdef SET_DIR_ATTRIB
/* messages of code for setting directory attributes */
static ZCONST char Far DirlistUidGidFailed[] =
  "warning:  cannot set UID %lu and/or GID %lu for %s\n";
static ZCONST char Far DirlistUtimeFailed[] =
  "warning:  cannot set modification, access times for %s\n";
#  ifndef NO_CHMOD
  static ZCONST char Far DirlistChmodFailed[] =
    "warning:  cannot set permissions for %s\n";
#  endif


int defer_dir_attribs(__G__ pd)
    __GDEF
    direntry **pd;
{
    uxdirattr *d_entry;

    d_entry = (uxdirattr *)malloc(sizeof(uxdirattr) + strlen(G.filename));
    *pd = (direntry *)d_entry;
    if (d_entry == (uxdirattr *)NULL) {
        return PK_MEM;
    }
    d_entry->fn = d_entry->fnbuf;
    strcpy(d_entry->fn, G.filename);

    d_entry->perms = G.pInfo->file_attr;

    d_entry->have_uidgid = get_extattribs(__G__ &(d_entry->u.t3),
                                          d_entry->uidgid);
    return PK_OK;
} /* end function defer_dir_attribs() */


int set_direc_attribs(__G__ d)
    __GDEF
    direntry *d;
{
    int errval = PK_OK;

    if (UxAtt(d)->have_uidgid &&
        chown(UxAtt(d)->fn, (uid_t)UxAtt(d)->uidgid[0],
              (gid_t)UxAtt(d)->uidgid[1]))
    {
        Info(slide, 0x201, ((char *)slide,
          LoadFarString(DirlistUidGidFailed),
          UxAtt(d)->uidgid[0], UxAtt(d)->uidgid[1], FnFilter1(d->fn)));
        if (!errval)
            errval = PK_WARN;
    }
    /* Skip restoring directory time stamps on user' request. */
    if (uO.D_flag <= 0) {
        /* restore directory timestamps */
        if (utime(d->fn, &UxAtt(d)->u.t2)) {
            Info(slide, 0x201, ((char *)slide,
              LoadFarString(DirlistUtimeFailed), FnFilter1(d->fn)));
            if (!errval)
                errval = PK_WARN;
        }
    }
#ifndef NO_CHMOD
    if (chmod(d->fn, filtattr(__G__ UxAtt(d)->perms))) {
        Info(slide, 0x201, ((char *)slide,
          LoadFarString(DirlistChmodFailed), FnFilter1(d->fn)));
        /* perror("chmod (file attributes) error"); */
        if (!errval)
            errval = PK_WARN;
    }
#endif /* !NO_CHMOD */
    return errval;
} /* end function set_direc_attribs() */

#endif /* SET_DIR_ATTRIB */




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
    sprintf((char *)slide, LoadFarString(CompiledWith),
#ifdef __GNUC__
      "gcc ", __VERSION__,
#else
      "(unknown compiler) ","",
#endif
      "Syllable",

#if defined(i486) || defined(__i486) || defined(__i486__) || defined(i386) || defined(__i386) || defined(__i386__)
      " (x86)",
#else
      " (unknown platform)",
#endif

#ifdef __DATE__
      " on ", __DATE__
#else
      "", ""
#endif
    );

    (*G.message)((zvoid *)&G, slide, (ulg)strlen((char *)slide), 0);

} /* end function version() */

#endif /* !SFX */



/*********************************/
/* AtheOS extra field functions  */
/*********************************/

/*
** Scan the extra fields in extra_field, and look for a AtheOS EF; return a
** pointer to that EF, or NULL if it's not there.
*/
static uch *scanAtheOSexfield(const uch *ef_ptr, unsigned ef_len)
{
    while( ef_ptr != NULL && ef_len >= EB_HEADSIZE ) {
        unsigned eb_id  = makeword(EB_ID + ef_ptr);
        unsigned eb_len = makeword(EB_LEN + ef_ptr);

        if (eb_len > (ef_len - EB_HEADSIZE)) {
            Trace((stderr,
              "scanAtheOSexfield: block length %u > rest ef_size %u\n", eb_len,
              ef_len - EB_HEADSIZE));
            break;
        }

        if (eb_id == EF_ATHEOS && eb_len >= EB_BEOS_HLEN) {
            return (uch *)ef_ptr;
        }

        ef_ptr += (eb_len + EB_HEADSIZE);
        ef_len -= (eb_len + EB_HEADSIZE);
    }

    return NULL;
}

/* Used by setAtheOSexfield():

Set a file/directory's attributes to the attributes passed in.

If set_file_attrs() fails, an error will be returned:

     EOK - no errors occurred

(other values will be whatever the failed function returned; no docs
yet, or I'd list a few)
*/
static int set_file_attrs( const char *name,
                           const unsigned char *attr_buff,
                           const off_t attr_size )
{
    int                  retval = EOK;
    unsigned char       *ptr;
    const unsigned char *guard;
    int                  fd;

    ptr   = (unsigned char *)attr_buff;
    guard = ptr + attr_size;

    fd = open(name, O_RDWR | O_NOTRAVERSE);
    if (fd < 0) {
        return errno; /* should it be -fd ? */
    }

    while (ptr < guard) {
        ssize_t              nError;
        struct attr_info     fa_info;
        const char          *attr_name;
        unsigned char       *attr_data;

        attr_name  = (char *)&(ptr[0]);
        ptr       += strlen(attr_name) + 1;

        /* The attr_info data is stored in little-endian format because the */
        /* Intel i386 port was here first.                                  */
        memcpy(&fa_info, ptr, sizeof(struct attr_info));
        ptr     += sizeof(struct attr_info);

        if (fa_info.ai_size < 0LL) {
            Info(slide, 0x201, ((char *)slide,
                 "warning: skipping attribute with invalid length (%Ld)\n",
                 fa_info.ai_size));
            break;
        }

        attr_data  = ptr;
        ptr       += fa_info.ai_size;

        if (ptr > guard) {
            /* We've got a truncated attribute. */
            Info(slide, 0x201, ((char *)slide,
                 "warning: truncated attribute\n"));
            break;
        }

        /* write_attr() doesn't return count of written bytes now (Syllable 0.5.3)... */
        nError = write_attr(fd, attr_name, O_TRUNC, fa_info.ai_type,
                            attr_data, 0, fa_info.ai_size);
        if (nError < 0) {
            Info(slide, 0x201, ((char *)slide,
                 "warning: error writing file attribute\n"));
        }
    }

    close(fd);
    return retval;
}

static void setAtheOSexfield(const char *path, uch *extra_field)
{
    uch *ptr       = extra_field;
    ush  id        = 0;
    ush  size      = 0;
    ulg  full_size = 0;
    uch  flags     = 0;
    uch *attrbuff  = NULL;
    int retval;

    if( extra_field == NULL ) {
        return;
    }

    /* Collect the data from the extra field buffer. */
    id        = makeword(ptr);    ptr += 2;   /* we don't use this... */
    size      = makeword(ptr);    ptr += 2;
    full_size = makelong(ptr);    ptr += 4;
    flags     = *ptr;             ptr++;

    /* Do a little sanity checking. */
    if (flags & EB_BE_FL_BADBITS) {
        /* corrupted or unsupported */
        Info(slide, 0x201, ((char *)slide,
          "Unsupported flags set for this AtheOS extra field, skipping.\n"));
        return;
    }
    if (size <= EB_BEOS_HLEN) {
        /* corrupted, unsupported, or truncated */
        Info(slide, 0x201, ((char *)slide,
             "AtheOS extra field is %d bytes, should be at least %d.\n", size,
             EB_BEOS_HLEN));
        return;
    }
    if (full_size < (size - EB_BEOS_HLEN)) {
        Info(slide, 0x201, ((char *)slide,
             "Skipping attributes: AtheOS extra field is %d bytes, "
             "data size is %ld.\n", size - EB_BEOS_HLEN, full_size));
        return;
    }

    /* Find the AtheOS file attribute data. */
    if (flags & EB_BE_FL_UNCMPR) {
        /* Uncompressed data */
        attrbuff = ptr;
    } else {
        /* Compressed data */
        attrbuff = (uch *)malloc( full_size );
        if (attrbuff == NULL) {
            /* No memory to uncompress attributes */
            Info(slide, 0x201, ((char *)slide,
                 "Can't allocate memory to uncompress file attributes.\n"));
            return;
        }

        retval = memextract(__G__ attrbuff, full_size,
                            ptr, size - EB_BEOS_HLEN);
        if( retval != PK_OK ) {
            /* error uncompressing attributes */
            Info(slide, 0x201, ((char *)slide,
                 "Error uncompressing file attributes.\n"));

            /* Some errors here might not be so bad; we should expect */
            /* some truncated data, for example.  If the data was     */
            /* corrupt, we should _not_ attempt to restore the attrs  */
            /* for this file... there's no way to detect what attrs   */
            /* are good and which are bad.                            */
            free (attrbuff);
            return;
        }
    }

    /* Now attempt to set the file attributes on the extracted file. */
    retval = set_file_attrs(path, attrbuff, (off_t)full_size);
    if (retval != EOK) {
        Info(slide, 0x201, ((char *)slide,
             "Error writing file attributes.\n"));
    }

    /* Clean up, if necessary */
    if (attrbuff != ptr) {
        free(attrbuff);
    }

    return;
}
