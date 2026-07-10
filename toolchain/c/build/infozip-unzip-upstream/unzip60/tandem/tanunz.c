/*
  Copyright (c) 1990-2008 Info-ZIP.  All rights reserved.

  See the accompanying file LICENSE, version 2000-Apr-09 or later
  (the contents of which are also included in unzip.h) for terms of use.
  If, for some reason, all these files are missing, the Info-ZIP license
  also may be found at:  ftp://ftp.info-zip.org/pub/infozip/license.html
*/
/*---------------------------------------------------------------------------

  tanunz.c

  Tandem/NSK routines for use with Info-ZIP's UnZip 5.3 and later.

  Contains:  do_wild()           <-- generic enough to put in fileio.c?
             ef_scan_for_tandem()
             open_outfile()
             mapattr()
             mapname()
             checkdir()
             mkdir()
             close_outfile()
             version()

  ---------------------------------------------------------------------------*/


#define UNZIP_INTERNAL
#include "unzip.h"

#include <tal.h>
#include "$system.zsysdefs.zsysc" nolist
#include <cextdecs> nolist
#include "tannsk.h"


char *in2ex OF((__GPRO__ char *));

static nsk_file_attrs *ef_scan_for_tandem (
    uch *ef_buf,
    unsigned ef_len
  );


static int created_dir;        /* used in mapname(), checkdir() */
static int renamed_fullpath;   /* ditto */


/****************************/
/* Strings used in tanunz.c */
/****************************/

static ZCONST char Far CannotDeleteOldFile[] =
  "error:  cannot delete old %s\n";
static ZCONST char Far CannotCreateFile[] = "error:  cannot create %s\n";


#ifndef SFX
/**********************/
/* Function do_wild() */  /* for porting: dir separator; match(ignore_case) */
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
    static char *intname;
    int isdir = 0;
    int pdosflag = 0;

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

        dirnamelen = strlen(wildspec);

        if ((dirname = (char *)malloc(dirnamelen+1)) == (char *)NULL) {
            Info(slide, 0x201, ((char *)slide,
              "warning:  cannot allocate wildcard buffers\n"));
             strncpy(matchname, wildspec, FILNAMSIZ);
             matchname[FILNAMSIZ-1] = '\0';
             return matchname;   /* but maybe filespec was not a wildcard */
        }
        strcpy(dirname, wildspec);
        wildname = wildspec;
        have_dirname = FALSE;

        if ((wild_dir = opendir(dirname)) != (DIR *)NULL) {
            while ((file = readdir(wild_dir)) != (struct dirent *)NULL) {
                Trace((stderr, "do_wild: readdir returns %s\n",
                  FnFilter1(file->d_name)));
                if (file->d_name[0] == '.' && wildname[0] != '.')
                    continue;  /* Unix: '*' and '?' do not match leading dot */
                if (match(file->d_name, wildname, 0 WISEP) && /* 0=case sens.*/
                    /* skip "." and ".." directory entries */
                    strcmp(file->d_name, ".") && strcmp(file->d_name, "..")) {
                    Trace((stderr, "do_wild: match() succeeds\n"));
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
        if (match(file->d_name, wildname, 0 WISEP)) {   /* 0 == case sens. */
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



/*********************************/
/* Function ef_scan_for_tandem() */
/*********************************/

static nsk_file_attrs *ef_scan_for_tandem(ef_buf, ef_len)
    uch *ef_buf;                /* buffer containing extra field */
    unsigned ef_len;            /* total length of extra field */
{
    unsigned eb_id;
    unsigned eb_len;

  /*---------------------------------------------------------------------------
    This function scans the extra field for EF_TANDEM
    -------------------------------------------------------------------------*/

    if (ef_buf == NULL)
      return NULL;

    while (ef_len >= EB_HEADSIZE) {
      eb_id = makeword(EB_ID + ef_buf);
      eb_len = makeword(EB_LEN + ef_buf);

      if (eb_len > (ef_len - EB_HEADSIZE)) {
          /* discovered some extra field inconsistency! */
          TTrace((stderr,
            "ef_scan_for_tandem: block length %u > rest ef_size %u\n", eb_len,
            ef_len - EB_HEADSIZE));
          break;
      }

      switch (eb_id) {
        case EF_TANDEM:
          return (nsk_file_attrs *)(char *)(ef_buf + EB_HEADSIZE);
          break;

        default:
          break;
      }

      /* Skip this extra field block */
      ef_buf += (eb_len + EB_HEADSIZE);
      ef_len -= (eb_len + EB_HEADSIZE);
  }

  return NULL;
}


/***************************/
/* Function open_outfile() */
/***************************/

int open_outfile(__G)           /* return 1 if fail */
    __GDEF
{
    int fdesc;
    short fnum, err, len;
    int priext, secext;
    short maxext, filecode, blocksize;

    #define alist_items 1
    #define vlist_bytes 2
    short alist[alist_items]={42};
    unsigned short vlist[alist_items];
    short extra, *err_item=&extra;
    nsk_file_attrs *znsk_attr;
    ulg eof, pages;
    char nsk_work[FILENAME_MAX + 1], *nsk_fname=&nsk_work[0];

#ifdef DLL
    if (G.redirect_data)
        return (redirect_outfile(__G) == FALSE);
#endif
    if (SSTAT(G.filename, &G.statbuf) == 0) {
        Trace((stderr, "open_outfile:  stat(%s) returns 0:  file exists\n",
          FnFilter1(G.filename)));
        if (unlink(G.filename) != 0) {
            Trace((stderr, "open_outfile:  existing file %s is read-only\n",
              FnFilter1(G.filename)));
            chmod(G.filename, S_IRUSR | S_IWUSR);
            Trace((stderr, "open_outfile:  %s now writable\n",
              FnFilter1(G.filename)));
            if (unlink(G.filename) != 0) {
                Info(slide, 0x401, ((char *)slide,
                  LoadFarString(CannotDeleteOldFile), FnFilter1(G.filename)));
                return 1;
            }
        }
        Trace((stderr, "open_outfile:  %s now deleted\n",
          FnFilter1(G.filename)));
    }

    /* Set up Tandem specific file information if present */
    znsk_attr = ef_scan_for_tandem(G.extra_field, G.lrec.extra_field_length);
    if (znsk_attr != NULL) {
        /* Set extent sizes */
        priext = znsk_attr->priext;
        secext  = znsk_attr->secext;
        maxext  = (int) znsk_attr->maxext;
        /* If original file was Enscribe and text then recreate as Edit */
        filecode = (znsk_attr->filetype != NSK_UNSTRUCTURED ?
                    (G.pInfo->textmode ? NSK_EDITFILECODE : NSK_UNSTRUCTURED) :
                    znsk_attr->filecode);
        blocksize = znsk_attr->block;
    } else {
        /* Try to work out some decent sizes based on how big the file is */
        eof = G.lrec.ucsize;
        pages = (eof/2048) + 2;
        if (pages <= 500) {
            priext = pages; /* fits into one extent */
            maxext = 16;
        } else {
            priext = 500; /* Try and fit into 500 page chunks */
            maxext = pages/(priext - 50); /* Allow for Enscribe overhead */
            if (maxext > 978) {
                priext = eof >> 10;         /* 512 equal extents */
                maxext = 978;                /* 2048 * 512 == 2^10 */
            }
        }
        secext = priext;
        filecode = (G.pInfo->textmode ? NSK_EDITFILECODE : NSK_UNSTRUCTURED);
        blocksize = TANDEM_BLOCKSIZE;
    }

    if ((fdesc = creat(G.filename,,priext,secext)) != -1){
        fnum = fdtogfn ((short)fdesc);
        err = (SETMODE (fnum, SET_FILE_BUFFERSIZE, blocksize) != CCE);
        err = (SETMODE (fnum, SET_FILE_BUFFERED, 0, 0) != CCE);
        err = (SETMODE (fnum, SET_FILE_BUFFERED, 0, 1) != CCE);
        err = (SETMODE (fnum, SET_FILE_MAXEXTENTS, maxext) != CCE);
        err = close(fdesc);

        vlist[0] = filecode;

        /* Note that FILE_ALTERLIST_ expects uppercase names */
        /* Need to call strlen and upshift                   */
        len = strlen(G.filename);
        err = STRING_UPSHIFT_(G.filename,
                              len,
                              nsk_fname,
                              len);

        err = FILE_ALTERLIST_(nsk_fname,
                              len,
                              alist,
                              alist_items,
                              vlist,
                              vlist_bytes,
                              ,
                              err_item);
    };

    G.outfile = fopen(G.filename, (G.pInfo->textmode ? FOPWT : FOPW));

    if (G.outfile == (FILE *)NULL) {
        Info(slide, 1, ((char *)slide, LoadFarString(CannotCreateFile),
          FnFilter1(G.filename)));
        return 1;
    }

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
        case TANDEM_:
        case UNIX_:
        case VMS_:
        case ACORN_:
        case ATARI_:
        case ATHEOS_:
        case BEOS_:
        case QDOS_:
            G.pInfo->file_attr = (unsigned)(tmp >> 16);
            if (G.pInfo->file_attr != 0 || !G.extra_field) {
                return 0;
            } else {
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

    /* for originating systems without concept of "group," "other," "system" */
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
    char *pp, *cp;                 /* character pointers */
    char *lastsemi=(char *)NULL;   /* pointer to last semi-colon in pathcomp */
    int error = MPN_OK;
    register unsigned workch;      /* hold the character being tested */


/*---------------------------------------------------------------------------
    Initialize various pointers and counters and stuff.
  ---------------------------------------------------------------------------*/

    if (G.pInfo->vollabel)
        return MPN_VOL_LABEL;   /* can't set disk volume labels on Tandem */

    /* can create path as long as not just freshening, or if user told us */
    G.create_dirs = (!uO.fflag || renamed);

    created_dir = FALSE;        /* not yet */

    /* user gave full pathname:  don't prepend rootpath */
    renamed_fullpath = (renamed && (*G.filename == '/'));

    if (checkdir(__G__ (char *)NULL, INIT) == MPN_NOMEM)
        return MPN_NOMEM;       /* initialize path buffer, unless no memory */

    /* TANDEM - call in2ex */
    pp = in2ex(__G__ G.filename);
    if (pp == (char *)NULL)
        return MPN_NOMEM;
    strcpy(G.filename, pp);
    free(pp);

    *pathcomp = '\0';           /* initialize translation buffer */
    pp = pathcomp;              /* point to translation buffer */
    /* directories have already been junked in in2ex() */
    cp = G.filename;            /* point to internal zipfile-member pathname */

/*---------------------------------------------------------------------------
    Begin main loop through characters in filename.
  ---------------------------------------------------------------------------*/

    while ((workch = (uch)*cp++) != 0) {

        switch (workch) { /* includes space char, let checkdir handle it */
            case TANDEM_DELIMITER: /* can assume -j flag not given */
                *pp = '\0';
                if (((error = checkdir(__G__ pathcomp, APPEND_DIR))
                     & MPN_MASK) > MPN_INF_TRUNC)
                    return error;
                pp = pathcomp;    /* reset conversion buffer for next piece */
                lastsemi = (char *)NULL; /* leave direct. semi-colons alone */
                break;

            case ';':             /* VMS version (or DEC-20 attrib?) */
                lastsemi = pp;
                *pp++ = ';';      /* keep for now; remove VMS ";##" */
                break;            /*  later, if requested */

            default:
                /* allow European characters in filenames: */
                if (isprint(workch) || (128 <= workch && workch <= 254))
                    *pp++ = (char)workch;
        } /* end switch */

    } /* end while loop */

/*---------------------------------------------------------------------------
    Report if directory was created (and no file to create:  filename ended
    in '/'), check name to be sure it exists, and combine path and name be-
    fore exiting.
  ---------------------------------------------------------------------------*/

    if (G.filename[strlen(G.filename) - 1] == TANDEM_DELIMITER) {
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
/* By using in2ex() to pre-process the filename we do not need to worry
   about the lengths of filename parts.  We just need to cope with mapping the
   pseudo file extensions properly of the actual filename (last part of name).
   e.g.  "tandem c" -> tandemc
         "revsion.h" -> revisioh
 */
{
    static int rootlen = 0;   /* length of rootpath */
    static char *rootpath;    /* user's "extract-to" directory */
    static char *buildpath;   /* full path (so far) to extracted file */
    static char *end;         /* pointer to end of buildpath ('\0') */

#   define FN_MASK   7
#   define FUNCTION  (flag & FN_MASK)

    char fname[FILENAME_MAX + 1];
    short fnamelen, extlen, trunclen, i;
    char ext[EXTENSION_MAX + 1];
    char *ptr;


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

        if (stat(buildpath, &G.statbuf)) {  /* path doesn't exist */
            if (!G.create_dirs) { /* told not to create (freshening) */
                free(buildpath);
                return MPN_INF_SKIP;    /* path doesn't exist: nothing to do */
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
        *end++ = TANDEM_DELIMITER;
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

        if (!uO.rflag  /* Do not add extension if asked */
            && parsename(pathcomp, fname, ext))
        {
            fnamelen = strlen(fname);
            extlen = strlen(ext);
            if (fnamelen+extlen > MAXFILEPARTLEN) {
                /* Doesn't fit.  Best approx is to use up to three characters
                   from extension and place these on the end of as much of the
                   start of the filename part as possible.
                 */
                if (extlen > EXTENSION_MAX)
                    extlen = EXTENSION_MAX;
                trunclen = MAXFILEPARTLEN - extlen;
                ptr = fname;
                for (i=0; i < trunclen; i++)
                    *end++ = *ptr++;
                ptr = ext;
                for (i=0; i < extlen; i++)
                    *end++ = *ptr++;
                *end = '\0';     /* mark end of string */
            }
            else {
                /* Just join parts end to end */
                ptr = fname;
                while ((*end = *ptr++) != '\0')
                    ++end;
                ptr = ext;
                while ((*end = *ptr++) != '\0')
                  ++end;
            }
        }
        else
            while ((*end = *pathcomp++) != '\0')
              ++end;

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
            if (tmproot[rootlen-1] == TANDEM_DELIMITER) {
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
            tmproot[rootlen++] = TANDEM_DELIMITER;
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



/********************/
/* Function mkdir() */
/********************/

int mkdir(path, mode)
const char *path;  /* both    */
mode_t mode;       /* ignored */
/*
 * returns:   0 - successful
 *           -1 - failed (errno not set, however)
 */
{
    return 0;
}


/****************************/
/* Function close_outfile() */
/****************************/

void close_outfile(__G)    /* GRR: change to return PK-style warning level */
    __GDEF
{
    union {
        iztimes t3;             /* mtime, atime, ctime */
        ztimbuf t2;             /* modtime, actime */
    } zt;
    ulg z_uidgid[2];
    unsigned eb_izux_flg;
    nsk_file_attrs *znsk_attr;
    short err;
    unsigned short len;

    #define alist_items 1
    #define vlist_bytes 2
    short alist[alist_items]={42     };
    unsigned short vlist[alist_items];
    short extra, *err_item=&extra;

    char nsk_work[FILENAME_MAX + 1], *nsk_fname=&nsk_work[0];

    fclose(G.outfile);

    /* Set up Tandem specific file information if present */
    znsk_attr = ef_scan_for_tandem(G.extra_field, G.lrec.extra_field_length);

    /* Currently we do not create 'proper' Tandem files in unzip - we only  */
    /* create unstructured (180) or edit (101) files.  This code allows     */
    /* some limited support for restoration of Tandem file attributes       */

    if (znsk_attr != NULL) {
      /* Reset File Code */
      if (!uO.bflag && !G.pInfo->textmode
          && znsk_attr->filetype == NSK_UNSTRUCTURED) {
        /* leave type 101 files alone */
        vlist[0] = znsk_attr->filecode;

        /* Note that FILE_ALTERLIST_ expect uppercase names and that        */
        /* G.lrec.filename_length is the length of original zipped filename */
        /* not any 'rename' name.  Need to re call strlen and upshift       */
        len = strlen(G.filename);
        err = STRING_UPSHIFT_(G.filename,
                              len,
                              nsk_fname,
                              len);

        err = FILE_ALTERLIST_(nsk_fname,
                              len,
                              alist,
                              alist_items,
                              vlist,
                              vlist_bytes,
                              ,
                              err_item);
      }
    }


/*---------------------------------------------------------------------------
    Convert from MSDOS-format local time and date to Unix-format 32-bit GMT
    time:  adjust base year from 1980 to 1970, do usual conversions from
    yy/mm/dd hh:mm:ss to elapsed seconds, and account for timezone and day-
    light savings time differences.  If we have a Unix extra field, however,
    we're laughing:  both mtime and atime are ours.  On the other hand, we
    then have to check for restoration of UID/GID.
  ---------------------------------------------------------------------------*/

    eb_izux_flg = (G.extra_field ? ef_scan_for_izux(G.extra_field,
                   G.lrec.extra_field_length, 0, G.lrec.last_mod_dos_datetime,
#ifdef IZ_CHECK_TZ
                   (G.tz_is_valid ? &(zt.t3) : NULL),
#else
                   &(zt.t3),
#endif
                   z_uidgid) : 0);
    if (eb_izux_flg & EB_UT_FL_MTIME) {
        TTrace((stderr, "\nclose_outfile:  Unix e.f. modif. time = %ld\n",
          zt.t3.mtime));
    } else {
        zt.t3.mtime = dos_to_unix_time(G.lrec.last_mod_dos_datetime);
    }
    if (eb_izux_flg & EB_UT_FL_ATIME) {
        TTrace((stderr, "close_outfile:  Unix e.f. access time = %ld\n",
          zt.t3.atime));
    } else {
        zt.t3.atime = zt.t3.mtime;
        TTrace((stderr, "\nclose_outfile:  modification/access times = %ld\n",
          zt.t3.mtime));
    }

/*---------------------------------------------------------------------------
    Change the file's last modified time to that stored in the zipfile.
    Not sure how (yet) or whether it's a good idea to set the last open time
  ---------------------------------------------------------------------------*/

    /* skip restoring time stamps on user's request */
    if (uO.D_flag <= 1) {
        /* set the file's access and modification times */
        if (utime(G.filename, &(zt.t2)))
            if (uO.qflag)
                Info(slide, 0x201, ((char *)slide,
                  "warning:  cannot set times for %s\n",
                  FnFilter1(G.filename)));
            else
                Info(slide, 0x201, ((char *)slide,
                  " (warning) cannot set times"));
    }

/*---------------------------------------------------------------------------
    Change the file permissions from default ones to those stored in the
    zipfile.
  ---------------------------------------------------------------------------*/

#ifndef NO_CHMOD
    if (chmod(G.filename, 0xffff & G.pInfo->file_attr))
        perror("chmod (file attributes) error");
#endif

/*---------------------------------------------------------------------------
       if -X option was specified and we have UID/GID info, restore it
       this must come after the file security and modtimes changes - since once
       we have secured the file to somebody else we cannot access it again.
  ---------------------------------------------------------------------------*/

    if (uO.X_flag && eb_izux_flg & EB_UX2_VALID) {
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

} /* end function close_outfile() */


#ifndef SFX

/************************/
/*  Function version()  */
/************************/

void version(__G)
    __GDEF
{

    /* Pyramid, NeXT have problems with huge macro expansion, too: no Info() */
    sprintf((char *)slide, LoadFarString(CompiledWith),

    "C ",
    "T9255D44 - (16OCT98)",
    "NonStop ",
    "(Tandem/NSK)",

#ifdef __DATE__
      " on ", __DATE__
#else
      "", ""
#endif
    );

    (*G.message)((zvoid *)&G, slide, (ulg)strlen((char *)slide), 0);

} /* end function version() */

#endif /* !SFX */
