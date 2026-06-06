#include <unistd.h>
#ifdef ftruncate
#undef ftruncate
#endif
int (*foo)(int, off_t) = ftruncate;
int main(void) { return 0; }
