/*[ADV]*/
#include <sys/mman.h>
#ifdef posix_madvise
#undef posix_madvise
#endif
int (*foo)(void *, size_t, int) = posix_madvise;
int main(void) { return 0; }
