/* Test whether a basic dladdr invocation works. */

#include <dlfcn.h>

#include "../basic.h"

// https://www.austingroupbugs.net/view.php?id=993
// https://www.austingroupbugs.net/view.php?id=1847
// POSIX accidentally standardized DL_info as DL_info_t. Try to use the old
// name, so implementations not up to date with issue #1847 aren't penalized for
// this mistake.
#if !defined(Dl_info_t) && (defined(__linux__) || defined(__GNU__) || defined(__FreeBSD__) || defined(__NetBSD__) || defined(__DragonFly__) || defined(__APPLE__) || defined(__minix__))
#define Dl_info_t Dl_info
#endif

int main(void)
{
	Dl_info_t info;
	if ( dladdr((void*) main, &info) < 0 )
		errx(1, "dladdr: %s", dlerror());
	return 0;
}
