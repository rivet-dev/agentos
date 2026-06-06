#include <wchar.h>
#ifdef vwscanf
#undef vwscanf
#endif
int (*foo)(const wchar_t *restrict, va_list) = vwscanf;
int main(void) { return 0; }
