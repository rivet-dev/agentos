#include <wordexp.h>
void foo(wordexp_t* bar)
{
	size_t *qux = &bar->we_wordc;
	(void) qux;
}
int main(void) { return 0; }
