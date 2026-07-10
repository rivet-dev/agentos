/*
  Copyright (c) 1990-2004 Info-ZIP.  All rights reserved.

  See the accompanying file LICENSE, version 2000-Apr-09 or later
  (the contents of which are also included in unzip.h) for terms of use.
  If, for some reason, all these files are missing, the Info-ZIP license
  also may be found at:  ftp://ftp.info-zip.org/pub/infozip/license.html
*/
/* beos.h -- A few handy things for the BeOS port.     */
/* (c) 1997 Chris Herborth (chrish@qnx.com)            */
/* This is covered under the usual Info-ZIP copyright. */

/* "#define EF_BE_FL_UNCMPR 0x01" has been moved into unzpriv.h */
#define EB_BE_FL_BADBITS    0xfe    /* bits currently undefined */

#define BEOS_ASSIGN_FILETYPE 1      /* call update_mime_info() */

/*
DR9 'Be' extra-field layout:

'Be'      - signature
ef_size   - size of data in this EF (little-endian unsigned short)
full_size - uncompressed data size (little-endian unsigned long)
flag      - flags (byte)
            flags & EB_BE_FL_UNCMPR     = the data is not compressed
            flags & EB_BE_FL_BADBITS    = the data is corrupted or we
                                          can't handle it properly
data      - compressed or uncompressed file attribute data

If flag & EB_BE_FL_UNCMPR, the data is not compressed; this optimisation is
necessary to prevent wasted space for files with small attributes (which
appears to be quite common on the Advanced Access DR9 release).  In this
case, there should be ( ef_size - EB_BEOS_HLEN ) bytes of data, and full_size
should equal ( ef_size - EB_BEOS_HLEN ).

If the data is compressed, there will be ( ef_size - EB_BEOS_HLEN ) bytes of
compressed data, and full_size bytes of uncompressed data.

If a file has absolutely no attributes, there will not be a 'Be' extra field.

The uncompressed data is arranged like this:

attr_name\0 - C string
struct attr_info (fields in big-endian format)
attr_data (length in attr_info.size)
*/
