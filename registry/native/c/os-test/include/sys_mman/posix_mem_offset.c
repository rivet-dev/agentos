/*[TYM]*/
#include <sys/mman.h>
#ifdef posix_mem_offset
#undef posix_mem_offset
#endif
int (*foo)(const void *restrict, size_t, off_t *restrict, size_t *restrict, int *restrict) = posix_mem_offset;
int main(void) { return 0; }
