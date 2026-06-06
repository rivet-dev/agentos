/*[TYM]*/
#include <sys/mman.h>
#ifdef posix_typed_mem_open
#undef posix_typed_mem_open
#endif
int (*foo)(const char *, int, int) = posix_typed_mem_open;
int main(void) { return 0; }
