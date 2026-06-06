#include <regex.h>
void foo(regex_t* bar)
{
	size_t *qux = &bar->re_nsub;
	(void) qux;
}
int main(void) { return 0; }
