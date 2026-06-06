/*[TYM]*/
#include <sys/mman.h>
#ifdef posix_typed_mem_get_info
#undef posix_typed_mem_get_info
#endif
int (*foo)(int, struct posix_typed_mem_info *) = posix_typed_mem_get_info;
int main(void) { return 0; }
