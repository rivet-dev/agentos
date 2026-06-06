#include <unistd.h>
#ifdef _exit
#undef _exit
#endif
 void (*foo)(int) = _exit;
int main(void) { return 0; }
