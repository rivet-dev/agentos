#include <sys/utsname.h>
void foo(struct utsname* bar)
{
	char *qux = bar->version;
	(void) qux;
}
int main(void) { return 0; }
