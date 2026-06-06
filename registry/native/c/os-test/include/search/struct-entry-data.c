/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <search.h>
void foo(struct entry* bar)
{
	void **qux = &bar->data;
	(void) qux;
}
int main(void) { return 0; }
