/* Test whether a basic iconv_close invocation works. */

#include <iconv.h>

#include "../basic.h"

int main(void)
{
	// de facto: Unfortunately POSIX fails to standardize names for the
	// available encodings, and fails to provide a way to find the available
	// names except iconv -l whose format is unspecified. That means that
	// conforming applications have no way to actually invoke this interface.
	// However, the basic names like UTF-8 and UTF-16LE are available everywhere
	// and I would argue that those basic names should be standardized. In this
	// test, we rely on the names. If any new implementations fail to use these
	// names, well yes that's allowed, but no, they should align with tradition.
	iconv_t conv = iconv_open("UTF-8", "UTF-16LE");
	if ( conv == (iconv_t) -1 )
		err(1, "iconv_open");
	if ( iconv_close(conv) != 0 )
		err(1, "iconv_close");
	return 0;
}
