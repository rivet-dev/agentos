#include <unistd.h>
#ifdef lseek
#undef lseek
#endif
off_t (*foo)(int, off_t, int) = lseek;
int main(void) { return 0; }
