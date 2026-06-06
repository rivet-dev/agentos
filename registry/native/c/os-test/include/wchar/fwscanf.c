#include <wchar.h>
#ifdef fwscanf
#undef fwscanf
#endif
int (*foo)(FILE *restrict, const wchar_t *restrict, ...) = fwscanf;
int main(void) { return 0; }
