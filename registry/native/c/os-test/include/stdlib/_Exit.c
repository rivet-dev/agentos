#include <stdlib.h>
#ifdef _Exit
#undef _Exit
#endif
 void (*foo)(int) = _Exit;
int main(void) { return 0; }
