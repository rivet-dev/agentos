/* Test whether a basic wmemset invocation works. */

#include <wchar.h>

#include "../basic.h"

int main(void)
{
	wchar_t buf[8];
	void* ptr = wmemset(buf, L'x', 8);
	if ( ptr != buf )
		errx(1, "wmemset did not return buf");
	for ( size_t i = 0; i < 8; i++ )
		if ( buf[i] != L'x' )
			err(1, "buf[%zu] != 'x'", i);
	return 0;
}
