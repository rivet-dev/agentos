/*
  Copyright (c) 1990-2004 Info-ZIP.  All rights reserved.

  See the accompanying file LICENSE, version 2000-Apr-09 or later
  (the contents of which are also included in unzip.h) for terms of use.
  If, for some reason, all these files are missing, the Info-ZIP license
  also may be found at:  ftp://ftp.info-zip.org/pub/infozip/license.html
*/
/* atheos.h -- A few handy things for the AtheOS port
 *
 * (c) 1997 Chris Herborth (chrish@qnx.com) - BeOS port
 * (c) 2004 Ruslan Nickolaev (nruslan@hotbox.ru) - AtheOS port
 *
 * This is covered under the usual Info-ZIP copyright
 */

#ifndef _ATHEOS_H_
#define _ATHEOS_H_

#define EB_BE_FL_BADBITS    0xfe    /* bits currently undefined */

/*
AtheOS 'At' extra-field layout:
(same structure as the BeOS 'Be' e.f. layout, only signature and internal
conventions of the file attribute data are different...)

'At'      - signature
ef_size   - size of data in this EF (little-endian unsigned short)
full_size - uncompressed data size (little-endian unsigned long)
flag      - flags (byte)
            flags & EB_BE_FL_UNCMPR     = the data is not compressed
            flags & EB_BE_FL_BADBITS    = the data is corrupted or we
                                          can't handle it properly
data      - compressed or uncompressed file attribute data

If flag & EB_BE_FL_UNCMPR, the data is not compressed; this optimisation is
necessary to prevent wasted space for files with small attributes. In this
case, there should be (ef_size - EB_BEOS_HLEN) bytes of data, and full_size
should equal (ef_size - EB_BEOS_HLEN).

If the data is compressed, there will be (ef_size - EB_BEOS_HLEN) bytes of
compressed data, and full_size bytes of uncompressed data.

If a file has absolutely no attributes, there will not be a 'At' extra field.

The uncompressed data is arranged like this:

attr_name\0 - C string
struct attr_info (fields in little-endian format)
attr_data (length in attr_info.ai_size)
*/

#endif /* _ATHEOS_H_ */
