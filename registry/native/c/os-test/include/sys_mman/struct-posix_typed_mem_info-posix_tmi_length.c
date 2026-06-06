/*[TYM]*/
#include <sys/mman.h>
void foo(struct posix_typed_mem_info* bar)
{
	size_t *qux = &bar->posix_tmi_length;
	(void) qux;
}
int main(void) { return 0; }
