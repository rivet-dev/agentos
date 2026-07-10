/*
  Copyright (c) 1990-2009 Info-ZIP.  All rights reserved.

  See the accompanying file LICENSE, version 2009-Jan-02 or later
  (the contents of which are also included in unzip.h) for terms of use.
  If, for some reason, all these files are missing, the Info-ZIP license
  also may be found at:  ftp://ftp.info-zip.org/pub/infozip/license.html
*/
/*---------------------------------------------------------------------------

  aosvs.c

  AOS/VS-specific routines for use with Info-ZIP's UnZip 5.2 and later.
[GRR:  copied from unix.c -> undoubtedly has unnecessary stuff: delete at will]

  Contains:  readdir()
             do_wild()
             open_outfile()
             mapattr()
             mapname()
             checkdir()
             close_outfile()
             version()             <-- GRR:  needs work!  (Unix, not AOS/VS)
             zvs_create()
             zvs_credir()
             ux_to_vs_name()
             dgdate()

  ---------------------------------------------------------------------------*/


#define UNZIP_INTERNAL
#include "unzip.h"
#include "aosvs/aosvs.h"
#include <packets/create.h>
#include <sys_calls.h>
#include <paru.h>

#define symlink(resname,linkname) \
  zvs_create(linkname,-1L,-1L,-1L,ux_to_vs_name(vs_resname,resname),$FLNK,-1,-1)
                                             *  file type */

#ifdef DIRENT
#  include <dirent.h>
#else
#  ifdef SYSV
#    ifdef SYSNDIR
#      include <sys/ndir.h>
#    else
#      include <ndir.h>
#    endif
#  else /* !SYSV */
#    ifndef NO_SYSDIR
#      include <sys/dir.h>
#    endif
#  endif /* ?SYSV */
#  ifndef dirent
#    define dirent direct
#  endif
#endif /* ?DIRENT */

static int            created_dir;          /* used in mapname(), checkdir() */
static int            renamed_fullpath;     /* ditto */

static ZEXTRAFLD      zzextrafld;           /* buffer for extra field containing
                                             *  ?FSTAT packet & ACL buffer */
static char           vs_resname[2*$MXPL];
static char           vs_path[2*$MXPL];     /* buf for AOS/VS pathname */
static char           Vs_path[512];         /* should be big enough [GRR: ?] */
static P_CTIM         zztimeblock;          /* time block for file creation */
static ZVSCREATE_STRU zzcreatepacket;       /* packet for sys_create(), any


/***************************/
/* Strings used in aosvs.c */
/***************************/

static ZCONST char Far CannotDeleteOldFile[] =
  "error:  cannot delete old %s\n";
static ZCONST char Far CannotCreateFile[] = "error:  cannot create %s\n";


#ifndef SFX
#ifdef NO_DIR                  /* for AT&T 3B1 */

#define opendir(path) fopen(path,"r")
#define closedir(dir) fclose(dir)
typedef FILE DIR;

/*
 *  Apparently originally by Rich Salz.
 *  Cleaned up and modified by James W. Birdsall.
 */
struct dirent *readdir(dirp)
    DIR *dirp;
{
    static struct dirent entry;

    if (dirp == NULL)
        return NULL;

    for (;;)
        if (fread(&entry, sizeof (struct dirent), 1, dirp) == 0)
            return (struct dirent *)NULL;
        else if (entry.d_ino)
            return &entry;

} /* end function readdir() */

#endif /* NO_DIR */


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
        if (match(file->d_name, wildname, 0 WISEP)) {   /* 0 ==  case sens. */
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





/***************************/
/* Function open_outfile() */
/***************************/

int open_outfile(__G)           /* return 1 if fail */
    __GDEF
{
    int errc = 1;    /* init to show no success with AOS/VS info */
    long dmm, ddd, dyy, dhh, dmin, dss;


#ifdef DLL
    if (G.redirect_data)
        return redirect_outfile(__G)==FALSE;
#endif

    if (stat(G.filename, &G.statbuf) == 0 && unlink(G.filename) < 0) {
        Info(slide, 0x401, ((char *)slide, LoadFarString(CannotDeleteOldFile),
          FnFilter1(G.filename)));
        return 1;
    }

/*---------------------------------------------------------------------------
    If the file didn't already exist, we created it earlier.  But we just
    deleted it, which we still had to do in case we are overwriting an exis-
    ting file.  So we must create it now, again, to set the creation time
    properly.  (The creation time is the best functional approximation of
    the Unix mtime.  Really!)

    If we stored this with an AOS/VS Zip that set the extra field to contain
    the ?FSTAT packet and the ACL, we should use info from the ?FSTAT call
    now.  Otherwise (or if that fails), we should create anyway as best we
    can from the normal Zip info.

    In theory, we should look through an entire series of extra fields that
    might exist for the same file, but we're not going to bother.  If we set
    up other types of extra fields, or if other environments we run into may
    add their own stuff to existing entries in Zip files, we'll have to.

    Note that all the packet types for sys_fstat() are the same size & mostly
    have the same structure, with some fields being unused, etc.  Ditto for
    sys_create().  Thus, we assume a normal one here, not a dir/cpd or device
    or IPC file, & make little adjustments as necessary.  We will set ACLs
    later (to reduce the chance of lacking access to what we create now); note
    that for links the resolution name should be stored in the ACL field (once
    we get Zip recognizing links OK).
  ---------------------------------------------------------------------------*/

    if (G.extra_field != NULL) {
        memcpy((char *) &zzextrafld, G.extra_field, sizeof(zzextrafld));
        if (!memcmp(ZEXTRA_HEADID, zzextrafld.extra_header_id,
                    sizeof(zzextrafld.extra_header_id))  &&
            !memcmp(ZEXTRA_SENTINEL, zzextrafld.extra_sentinel),
                    sizeof(zzextrafld.extra_sentinel))
        {
            zzcreatepacket.norm_create_packet.cftyp_format =
              zzextrafld.fstat_packet.norm_fstat_packet.styp_format;
            zzcreatepacket.norm_create_packet.cftyp_entry =
              zzextrafld.fstat_packet.norm_fstat_packet.styp_type;

            /* for DIRS/CPDs, the next one will give the hash frame size; for
             * IPCs it will give the port number */
            zzcreatepacket.norm_create_packet.ccps =
              zzextrafld.fstat_packet.norm_fstat_packet.scps;

            zzcreatepacket.norm_create_packet.ctim = &zztimeblock;
            zztimeblock.tcth = zzextrafld.fstat_packet.norm_fstat_packet.stch;

            /* access & modification times default to current */
            zztimeblock.tath.long_time = zztimeblock.tmth.long_time = -1;

            /* give it current process's ACL unless link; then give it
             * resolution name */
            zzcreatepacket.norm_create_packet.cacp = (char *)(-1);

            if (zzcreatepacket.norm_create_packet.cftyp_entry == $FLNK)
                zzcreatepacket.norm_create_packet.cacp = zzextrafld.aclbuf;

            zzcreatepacket.dir_create_packet.cmsh =
              zzextrafld.fstat_packet.dir_fstat_packet.scsh;
            if (zzcreatepacket.norm_create_packet.cftyp_entry != $FCPD) {
                /* element size for normal files */
                zzcreatepacket.norm_create_packet.cdel =
                  zzextrafld.fstat_packet.norm_fstat_packet.sdeh;
            }
            zzcreatepacket.norm_create_packet.cmil =
              zzextrafld.fstat_packet.norm_fstat_packet.smil;

            if ((errc = sys_create(ux_to_vs_name(vs_path, G.filename),
                 &zzcreatepacket)) != 0)
                Info(slide, 0x201, ((char *)slide,
                  "error creating %s with AOS/VS info -\n\
                  will try again with ordinary Zip info\n",
                  FnFilter1(G.filename)));
        }
    }

    /* do it the hard way if no AOS/VS info was stored or if we had problems */
    if (errc) {
        /* skip restoring time stamps on user's request */
        if (uO.D_flag <= 1) {
            dyy = (G.lrec.last_mod_dos_datetime >> 25) + 1980;
            dmm = (G.lrec.last_mod_dos_datetime >> 21) & 0x0f;
            ddd = (G.lrec.last_mod_dos_datetime >> 16) & 0x1f;
            dhh = (G.lrec.last_mod_dos_datetime >> 11) & 0x1f;
            dmin = (G.lrec.last_mod_dos_datetime >> 5) & 0x3f;
            dss = (G.lrec.last_mod_dos_datetime << 1) & 0x3e;
        }

        if (zvs_create(G.filename,
            (uO.D_flag <= 1
             ? (((ulg)dgdate(dmm, ddd, dyy)) << 16) |
               (dhh*1800L + dmin*30L + dss/2L)
             : -1L),
            -1L, -1L, (char *) -1, -1, -1, -1))
        {
            Info(slide, 0x201, ((char *)slide, "error: %s: cannot create\n",
              FnFilter1(G.filename)));
            return 1;
        }
    }

    Trace((stderr, "open_outfile:  doing fopen(%s) for writing\n",
      FnFilter1(G.filename)));
    if ((G.outfile = fopen(G.filename, FOPW)) == (FILE *)NULL) {
        Info(slide, 0x401, ((char *)slide, LoadFarString(CannotCreateFile),
          FnFilter1(G.filename)));
        return 1;
    }
    Trace((stderr, "open_outfile:  fopen(%s) for writing succeeded\n",
      FnFilter1(G.filename)));

#ifdef USE_FWRITE
#ifdef _IOFBF  /* make output fully buffered (works just about like write()) */
    setvbuf(G.outfile, (char *)slide, _IOFBF, WSIZE);
#else
    setbuf(G.outfile, (char *)slide);
#endif
#endif /* USE_FWRITE */
    return 0;

} /* end function open_outfile() */





/**********************/
/* Function mapattr() */
/**********************/

int mapattr(__G)
    __GDEF
{
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
            G.pInfo->file_attr = (unsigned)(tmp >> 16);
            if (G.pInfo->file_attr != 0 || !G.extra_field) {
                return 0;
            } else {
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
                 * (For ASI Unix e.f., an experimental remap from the e.f.
                 * mode value IS already provided!)
                 */
                ush ebID;
                unsigned ebLen;
                uch *ef = G.extra_field;
                unsigned ef_len = G.crec.extra_field_length;
                int r = FALSE;

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
                if (!r)
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
            if ((G.pInfo->file_attr & 0700) == (unsigned)(0400 | tmp<<6))
                /* keep previous G.pInfo->file_attr setting, when its "owner"
                 * part appears to be consistent with DOS attribute flags!
                 */
                return 0;
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
        return MPN_VOL_LABEL;   /* can't set disk volume labels in Unix */

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
#ifdef SHORT_NAMES
        char *old_end = end;
#endif

        Trace((stderr, "appending dir segment [%s]\n", FnFilter1(pathcomp)));
        while ((*end = *pathcomp++) != '\0')
            ++end;
#ifdef SHORT_NAMES   /* path components restricted to 14 chars, typically */
        if ((end-old_end) > FILENAME_MAX)  /* GRR:  proper constant? */
            *(end = old_end + FILENAME_MAX) = '\0';
#endif

        /* GRR:  could do better check, see if overrunning buffer as we go:
         * check end-buildpath after each append, set warning variable if
         * within 20 of FILNAMSIZ; then if var set, do careful check when
         * appending.  Clear variable when begin new path. */

        /* next check: need to append '/', at least one-char name, '\0' */
        if ((end-buildpath) > FILNAMSIZ-3)
            too_long = TRUE;                    /* check if extracting dir? */
        /* for AOS/VS, try to create so as to not use searchlist: */
        if ( /*SSTAT(buildpath, &G.statbuf)*/ 1) {
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
            /* create the directory */
            if (zvs_credir(buildpath,-1L,-1L,-1L,(char *) -1,-1,0L,-1,-1) == -1)
            {
                Info(slide, 1, ((char *)slide,
                  "checkdir error:  cannot create %s\n\
                 %s\n\
                 unable to process %s.\n",
                  FnFilter2(buildpath),
                  strerror(errno),
                  FnFilter1(G.filename)));
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
#ifdef SHORT_NAMES
        char *old_end = end;
#endif

        Trace((stderr, "appending filename [%s]\n", FnFilter1(pathcomp)));
        while ((*end = *pathcomp++) != '\0') {
            ++end;
#ifdef SHORT_NAMES  /* truncate name at 14 characters, typically */
            if ((end-old_end) > FILENAME_MAX)      /* GRR:  proper constant? */
                *(end = old_end + FILENAME_MAX) = '\0';
#endif
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

/* GRR:  for VMS and TOPS-20, add up to 13 to strlen */

    if (FUNCTION == INIT) {
        Trace((stderr, "initializing buildpath to "));
        if ((buildpath = (char *)malloc(strlen(G.filename)+rootlen+1))
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
        Trace((stderr, "initializing root path to [%s]\n",
          FnFilter1(pathcomp)));
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
            if (rootlen > 0 && (SSTAT(tmproot, &G.statbuf) ||
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
                if (zvs_credir(tmproot,-1L,-1L,-1L,(char *) -1,-1,0L,-1,-1)
                    == -1)
                {
                    Info(slide, 1, ((char *)slide,
                      "checkdir:  cannot create extraction directory: %s\n\
           %s\n",
                      FnFilter1(tmproot), strerror(errno)));
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
#endif /* SYMLINKS */

    fclose(G.outfile);

/*---------------------------------------------------------------------------
    Change the file permissions from default ones to those stored in the
    zipfile.
  ---------------------------------------------------------------------------*/

#ifndef NO_CHMOD
    if (chmod(G.filename, 0xffff & G.pInfo->file_attr))
        perror("chmod (file attributes) error");
#endif

/*---------------------------------------------------------------------------
    AOS/VS only allows setting file times at creation but has its own permis-
    sions scheme which is better invoked here if the necessary information
    was in fact stored.  In theory, we should look through an entire series
    of extra fields that might exist for the same file, but we're not going
    to bother.  If we set up other types of extra fields, or if we run into
    other environments that add their own stuff to existing entries in ZIP
    files, we'll have to.  NOTE:  already copied extra-field stuff into
    zzextrafld structure when file was created.
  ---------------------------------------------------------------------------*/

    if (G.extra_field != NULL) {
        if (!memcmp(ZEXTRA_HEADID, zzextrafld.extra_header_id,
                    sizeof(zzextrafld.extra_header_id))  &&
            !memcmp(ZEXTRA_SENTINEL, zzextrafld.extra_sentinel,
                    sizeof(zzextrafld.extra_sentinel))  &&
            zzextrafld.fstat_packet.norm_fstat_packet.styp_type != $FLNK)
            /* (AOS/VS links don't have ACLs) */
        {
            /* vs_path was set (in this case) when we created the file */
            if (sys_sacl(vs_path, zzextrafld.aclbuf)) {
                Info(slide, 0x201, ((char *)slide,
                  "error: cannot set ACL for %s\n", FnFilter1(G.filename)));
                perror("sys_sacl()");
            }
        }
    }
} /* end function close_outfile() */




#ifndef SFX

/************************/
/*  Function version()  */
/************************/

void version(__G)
    __GDEF
{
#if (defined(__GNUC__) && defined(NX_CURRENT_COMPILER_RELEASE))
    char cc_namebuf[40];
    char cc_versbuf[40];
#else
#if (defined(CRAY) && defined(_RELEASE))
    char cc_versbuf[40];
#endif
#endif
#if ((defined(CRAY) || defined(cray)) && defined(_UNICOS))
    char os_namebuf[40];
#else
#if defined(__NetBSD__)
    char os_namebuf[40];
#endif
#endif

    /* Pyramid, NeXT have problems with huge macro expansion, too:  no Info() */
    sprintf((char *)slide, LoadFarString(CompiledWith),

#ifdef __GNUC__
#  ifdef NX_CURRENT_COMPILER_RELEASE
      (sprintf(cc_namebuf, "NeXT DevKit %d.%02d ",
        NX_CURRENT_COMPILER_RELEASE/100, NX_CURRENT_COMPILER_RELEASE%100),
       cc_namebuf),
      (strlen(__VERSION__) > 8)? "(gcc)" :
        (sprintf(cc_versbuf, "(gcc %s)", __VERSION__), cc_versbuf),
#  else
      "gcc ", __VERSION__,
#  endif
#else
#  if defined(CRAY) && defined(_RELEASE)
      "cc ", (sprintf(cc_versbuf, "version %d", _RELEASE), cc_versbuf),
#  else
#  ifdef __VERSION__
#   ifndef IZ_CC_NAME
#    define IZ_CC_NAME "cc "
#   endif
      IZ_CC_NAME, __VERSION__
#  else
#   ifndef IZ_CC_NAME
#    define IZ_CC_NAME "cc"
#   endif
      IZ_CC_NAME, "",
#  endif
#  endif
#endif /* ?__GNUC__ */

#ifndef IZ_OS_NAME
#  define IZ_OS_NAME "Unix"
#endif
      IZ_OS_NAME,

#if defined(sgi) || defined(__sgi)
      " (Silicon Graphics IRIX)",
#else
#ifdef sun
#  ifdef sparc
#    ifdef __SVR4
      " (Sun SPARC/Solaris)",
#    else /* may or may not be SunOS */
      " (Sun SPARC)",
#    endif
#  else
#  if defined(sun386) || defined(i386)
      " (Sun 386i)",
#  else
#  if defined(mc68020) || defined(__mc68020__)
      " (Sun 3)",
#  else /* mc68010 or mc68000:  Sun 2 or earlier */
      " (Sun 2)",
#  endif
#  endif
#  endif
#else
#ifdef __hpux
      " (HP/UX)",
#else
#ifdef __osf__
      " (DEC OSF/1)",
#else
#ifdef _AIX
      " (IBM AIX)",
#else
#ifdef aiws
      " (IBM RT/AIX)",
#else
#if defined(CRAY) || defined(cray)
#  ifdef _UNICOS
      (sprintf(os_namebuf, " (Cray UNICOS release %d)", _UNICOS), os_namebuf),
#  else
      " (Cray UNICOS)",
#  endif
#else
#if defined(uts) || defined(UTS)
      " (Amdahl UTS)",
#else
#ifdef NeXT
#  ifdef mc68000
      " (NeXTStep/black)",
#  else
      " (NeXTStep for Intel)",
#  endif
#else              /* the next dozen or so are somewhat order-dependent */
#ifdef LINUX
#  ifdef __ELF__
      " (Linux ELF)",
#  else
      " (Linux a.out)",
#  endif
#else
#ifdef MINIX
      " (Minix)",
#else
#ifdef M_UNIX
      " (SCO Unix)",
#else
#ifdef M_XENIX
      " (SCO Xenix)",
#else
#ifdef __NetBSD__
#  ifdef NetBSD0_8
      (sprintf(os_namebuf, " (NetBSD 0.8%c)", (char)(NetBSD0_8 - 1 + 'A')),
       os_namebuf),
#  else
#  ifdef NetBSD0_9
      (sprintf(os_namebuf, " (NetBSD 0.9%c)", (char)(NetBSD0_9 - 1 + 'A')),
       os_namebuf),
#  else
#  ifdef NetBSD1_0
      (sprintf(os_namebuf, " (NetBSD 1.0%c)", (char)(NetBSD1_0 - 1 + 'A')),
       os_namebuf),
#  else
      (BSD4_4 == 0.5)? " (NetBSD before 0.9)" : " (NetBSD 1.1 or later)",
#  endif
#  endif
#  endif
#else
#ifdef __FreeBSD__
      (BSD4_4 == 0.5)? " (FreeBSD 1.x)" : " (FreeBSD 2.0 or later)",
#else
#ifdef __bsdi__
      (BSD4_4 == 0.5)? " (BSD/386 1.0)" : " (BSD/386 1.1 or later)",
#else
#ifdef __386BSD__
      (BSD4_4 == 1)? " (386BSD, post-4.4 release)" : " (386BSD)",
#else
#if defined(i486) || defined(__i486) || defined(__i486__)
      " (Intel 486)",
#else
#if defined(i386) || defined(__i386) || defined(__i386__)
      " (Intel 386)",
#else
#ifdef pyr
      " (Pyramid)",
#else
#ifdef ultrix
#  ifdef mips
      " (DEC/MIPS)",
#  else
#  ifdef vax
      " (DEC/VAX)",
#  else /* __alpha? */
      " (DEC/Alpha)",
#  endif
#  endif
#else
#ifdef gould
      " (Gould)",
#else
#ifdef MTS
      " (MTS)",
#else
#ifdef __convexc__
      " (Convex)",
#else
      "",
#endif /* Convex */
#endif /* MTS */
#endif /* Gould */
#endif /* DEC */
#endif /* Pyramid */
#endif /* 386 */
#endif /* 486 */
#endif /* 386BSD */
#endif /* BSDI BSD/386 */
#endif /* NetBSD */
#endif /* FreeBSD */
#endif /* SCO Xenix */
#endif /* SCO Unix */
#endif /* Minix */
#endif /* Linux */
#endif /* NeXT */
#endif /* Amdahl */
#endif /* Cray */
#endif /* RT/AIX */
#endif /* AIX */
#endif /* OSF/1 */
#endif /* HP/UX */
#endif /* Sun */
#endif /* SGI */

#ifdef __DATE__
      " on ", __DATE__
#else
      "", ""
#endif
    );

    (*G.message)((zvoid *)&G, slide, (ulg)strlen((char *)slide), 0);

} /* end function version() */

#endif /* !SFX */





/* ===================================================================
 * ZVS_CREATE()
 * Function to create a file with specified times.  The times should be sent
 * as long ints in DG time format; use -1 to set to the current times.  You
 * may also specify a pointer to the ACL, the file type (see PARU.H, and do
 * not specify dirs or links), the element size, and the max index level.
 * For all of these parameters you may use -1 to specify the default.
 *
 * Returns 0 if no error, or the error code returned by ?CREATE.
 *
 *    HISTORY:
 *        15-dec-93 dbl
 *        31-may-94 dbl: added call to convert pathname to AOS/VS
 *
 *
 */

int zvs_create(ZCONST char *fname, long cretim, long modtim, long acctim,
               char *pacl, int ftyp, int eltsize, int maxindlev)
{
    P_CREATE    pcr_stru;
    P_CTIM      pct_stru;

    pcr_stru.cftyp_format = 0;           /* unspecified record format */
    if (ftyp == -1)                      /* default file type to UNX */
        pcr_stru.cftyp_entry = $FUNX;
    else
        pcr_stru.cftyp_entry = ftyp;
    pcr_stru.ctim = &pct_stru;
    pcr_stru.cacp = pacl;
    pcr_stru.cdel = eltsize;
    pcr_stru.cmil = maxindlev;

    pct_stru.tcth.long_time = cretim;
    pct_stru.tath.long_time = acctim;
    pct_stru.tmth.long_time = modtim;

    return (sys_create(ux_to_vs_name(Vs_path, fname), &pcr_stru));

} /* end zvs_create() */



/* ===================================================================
 * ZVS_CREDIR()
 * Function to create a dir as specified.  The times should be sent
 * as long ints in DG time format; use -1 to set to the current times.  You
 * may also specify a pointer to the ACL, the file type (either $FDIR or $FCPD; see PARU.H),
 * the max # blocks (if a CPD), the hash frame size, and the max index level.
 * For all of these parameters (except for the CPD's maximum blocks),
 * you may use -1 to specify the default.
 *
 * (The System Call Dictionary says both that you may specify a
 * maximum-index-level value up to the maximum, with 0 for a contiguous
 * directory, and that 3 is always used for this whatever you specify.)
 *
 * If you specify anything other than CPD for the file type, DIR will
 * be used.
 *
 * Returns 0 if no error, or the error code returned by ?CREATE.
 *
 *    HISTORY:
 *        1-jun-94 dbl
 *
 *
 */

int zvs_credir(ZCONST char *dname, long cretim, long modtim, long acctim,
               char *pacl, int ftyp, long maxblocks, int hashfsize,
               int maxindlev)
{
    P_CREATE_DIR    pcr_stru;
    P_CTIM          pct_stru;

    if (ftyp != $FCPD)                      /* default file type to UNX */
        pcr_stru.cftyp_entry = $FDIR;
    else
    {
        pcr_stru.cftyp_entry = ftyp;
        pcr_stru.cmsh = maxblocks;
    }

    pcr_stru.ctim = &pct_stru;
    pcr_stru.cacp = pacl;
    pcr_stru.chfs = hashfsize;
    pcr_stru.cmil = maxindlev;

    pct_stru.tcth.long_time = cretim;
    pct_stru.tath.long_time = acctim;
    pct_stru.tmth.long_time = modtim;

    return (sys_create(ux_to_vs_name(Vs_path, dname), &pcr_stru));

} /* end zvs_credir() */



/* ===================================================================
 * UX_TO_VS_NAME() - makes a somewhat dumb pass at converting a Unix
 *           filename to an AOS/VS filename.  This should
 *           be just about adequate to handle the results
 *           of similarly-simple AOS/VS-to-Unix conversions
 *           in the ZIP program.  It does not guarantee a
 *           legal AOS/VS filename for every Unix filename;
 *           conspicuous examples would be names with
 *           embedded ./ and ../ (which will receive no
 *           special treatment).
 *
 *       RETURNS: pointer to the result (which is an input parameter)
 *
 *       NOTE: calling code is responsible for making sure
 *           the output buffer is big enough!
 *
 *       HISTORY:
 *           31-may-94 dbl
 *
 */
char *ux_to_vs_name(char *outname, ZCONST char *inname)
{
    ZCONST char *ip=inname, *op=outname;


    if (ip[0] == '.') {
        if (ip[1] == '/') {
            *(op++) = '=';
            ip += 2;
        } else if (ip[1] == '.'  &&  ip[2] == '/') {
            *(op++) = '^';
            ip += 3;
        }
    }

    do {
        if (*ip == '/')
            *(op++) = ':';
        else if (strchr(
           "0123456789_$?.abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ",
           *ip) != NULL)
        {
            *(op++) = *ip;
        } else
            *(op++) = '?';

    } while (*(ip++) != '\0');

    return outname;

} /* end ux_to_vs_name() */



/* =================================================================== */

/* DGDATE
   Two functions do encode/decode dates in DG system format.

   Usage:
    long value,year,month,day;

    value=dgdate(month,day,year);
    undgdate(value,&month,&day,&year);   [GRR:  not used in UnZip: removed]

   Notes:

   1. DG date functions only work on dates within the range
      Jan 1, 1968 through Dec 31, 2099.  I have tested these
      functions through the same range with exact agreement.
      For dates outside of that range, the DG system calls
      may return different values than these functions.

   2. dgundate() accepts values between 0 and 48213 inclusive.
      These correspond to 12/31/1967 and 12/31/2099.

   3. Both functions assume the data is in the native OS byte
      order.  So if you're reading or writing these fields from
      a file that has been passed between AOS/VS and PC-DOS you
      will need to swap byte order.

   4. With reference to byte order, the entire range of values
      supported by these functions will fit into an unsigned
      short int.  In most cases the input or output will be
      in that variable type.  You are better off casting the
      value to/from unsigned short so you only need to concern
      yourself with swapping two bytes instead of four.

  Written by: Stanley J. Gula
              US&T, Inc.
              529 Main Street, Suite 1
              Indian Orchard, MA 01151
              (413)-543-3672
              Copyright (c) 1990 US&T, Inc.
              All rights reserved.

              I hereby release these functions into the public
              domain.  You may use these routines freely as long
              as the US&T copyright remains intact in the source
              code.

              Stanley J. Gula     July 24, 1990
*/

long motable[13]={0,31,59,90,120,151,181,212,243,273,304,334,365};

long yrtable[132]={
      366,  731, 1096, 1461, 1827, 2192, 2557, 2922, 3288, 3653,
     4018, 4383, 4749, 5114, 5479, 5844, 6210, 6575, 6940, 7305,
     7671, 8036, 8401, 8766, 9132, 9497, 9862,10227,10593,10958,
    11323,11688,12054,12419,12784,13149,13515,13880,14245,14610,
    14976,15341,15706,16071,16437,16802,17167,17532,17898,18263,
    18628,18993,19359,19724,20089,20454,20820,21185,21550,21915,
    22281,22646,23011,23376,23742,24107,24472,24837,25203,25568,
    25933,26298,26664,27029,27394,27759,28125,28490,28855,29220,
    29586,29951,30316,30681,31047,31412,31777,32142,32508,32873,
    33238,33603,33969,34334,34699,35064,35430,35795,36160,36525,
    36891,37256,37621,37986,38352,38717,39082,39447,39813,40178,
    40543,40908,41274,41639,42004,42369,42735,43100,43465,43830,
    44196,44561,44926,45291,45657,46022,46387,46752,47118,47483,
    47848,48213};

/* Given y,m,d return # of days since 12/31/67 */
long int dgdate(short mm, short dd, short yy)
{
    long int temp;
    short ytmp;

    if (mm<1 || mm>12 || dd<1 || dd>31 || yy<1968 || yy>2099)
        return 0L;

    /* Figure in whole years since 1968 + whole months plus days */
    temp=365L*(long)(yy-1968) + motable[mm-1] + (long)dd;

    /* Adjust for leap years - note we don't account for skipped leap
       year in years divisible by 1000 but not by 4000.  We're correct
       through the year 2099 */
    temp+=(yy-1965)/4;

    /* Correct for this year */
    /* In leap years, if date is 3/1 or later, bump */
    if ((yy%4==0) && (mm>2))
        temp++;

    return temp;
}
