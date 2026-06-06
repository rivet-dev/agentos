#include <inttypes.h>
void foo(imaxdiv_t* bar)
{
	intmax_t *qux = &bar->rem;
	(void) qux;
}
int main(void) { return 0; }
