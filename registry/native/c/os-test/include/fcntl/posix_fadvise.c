/*[ADV]*/
#include <fcntl.h>
#ifdef posix_fadvise
#undef posix_fadvise
#endif
int (*foo)(int, off_t, off_t, int) = posix_fadvise;
int main(void) { return 0; }
