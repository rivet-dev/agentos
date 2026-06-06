/*[ADV]*/
#include <fcntl.h>
#ifdef posix_fallocate
#undef posix_fallocate
#endif
int (*foo)(int, off_t, off_t) = posix_fallocate;
int main(void) { return 0; }
