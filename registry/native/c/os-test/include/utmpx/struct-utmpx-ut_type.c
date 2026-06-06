/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <utmpx.h>
void foo(struct utmpx* bar)
{
	short *qux = &bar->ut_type;
	(void) qux;
}
int main(void) { return 0; }
