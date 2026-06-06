/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <ndbm.h>
void foo(datum* bar)
{
	void **qux = &bar->dptr;
	(void) qux;
}
int main(void) { return 0; }
